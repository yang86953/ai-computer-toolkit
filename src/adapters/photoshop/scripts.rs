//! Photoshop 内部命名脚本。公共 facade 只接收结构化领域输入。

pub(super) const DOCUMENTS_SCRIPT: &str = r#"(function () {
    function esc(value) {
        return String(value).replace(/\\/g, "\\\\").replace(/\"/g, "\\\"")
            .replace(/\r/g, "\\r").replace(/\n/g, "\\n").replace(/\t/g, "\\t");
    }
    function q(value) { return "\"" + esc(value) + "\""; }
    var items = [];
    for (var i = 0; i < app.documents.length; i++) {
        var d = app.documents[i];
        var path = null;
        try { path = d.fullName.fsName; } catch (_) {}
        items.push("{" +
            "\"id\":" + d.id + "," +
            "\"name\":" + q(d.name) + "," +
            "\"path\":" + (path === null ? "null" : q(path)) + "," +
            "\"width\":" + d.width.as("px") + "," +
            "\"height\":" + d.height.as("px") + "," +
            "\"resolution\":" + d.resolution + "," +
            "\"layerCount\":" + d.layers.length + "," +
            "\"saved\":" + (d.saved ? "true" : "false") + "," +
            "\"active\":" + (app.activeDocument.id === d.id ? "true" : "false") +
        "}");
    }
    return "{\"documents\":[" + items.join(",") + "]}";
})()"#;

pub(super) const CREATE_SCRIPT: &str = r#"(function () {
    function esc(value) { return String(value).replace(/\\/g, "\\\\").replace(/\"/g, "\\\""); }
    var previous = null;
    try { previous = app.activeDocument; } catch (_) {}
    var d = app.documents.add(__WIDTH__, __HEIGHT__, __RESOLUTION__, __NAME__, NewDocumentMode.RGB, DocumentFill.TRANSPARENT);
    d.activeLayer.name = "__TOOLKIT_EMPTY__";
    var result = "{\"id\":" + d.id + ",\"name\":\"" + esc(d.name) + "\",\"width\":" + d.width.as("px") + ",\"height\":" + d.height.as("px") + ",\"resolution\":" + d.resolution + "}";
    if (previous !== null) { app.activeDocument = previous; }
    return result;
})()"#;

pub(super) const APPLY_SCRIPT: &str = r#"(function () {
    function esc(value) { return String(value).replace(/\\/g, "\\\\").replace(/\"/g, "\\\""); }
    function color(hex) {
        var c = new SolidColor();
        c.rgb.red = parseInt(hex.substr(1, 2), 16);
        c.rgb.green = parseInt(hex.substr(3, 2), 16);
        c.rgb.blue = parseInt(hex.substr(5, 2), 16);
        return c;
    }
    function findDocument(id) {
        for (var i = 0; i < app.documents.length; i++) { if (app.documents[i].id === id) return app.documents[i]; }
        throw new Error("STALE_SESSION");
    }
    function findLayer(container, id) {
        for (var i = 0; i < container.layers.length; i++) {
            var layer = container.layers[i];
            if (layer.id === id) return layer;
            if (layer.typename === "LayerSet") { var nested = findLayer(layer, id); if (nested !== null) return nested; }
        }
        return null;
    }
    var doc = findDocument(__DOCUMENT_ID__);
    var previousDocument = null;
    var previousLayerId = null;
    var rollbackState = null;
    try { previousDocument = app.activeDocument; } catch (_) {}
    try { previousLayerId = doc.activeLayer.id; } catch (_) {}
    var previousUnits = app.preferences.rulerUnits;
    function addRect(name, x, y, width, height, fill, opacity, rotation) {
        var layer = doc.artLayers.add(); layer.name = name;
        doc.selection.select([[x,y],[x+width,y],[x+width,y+height],[x,y+height]], SelectionType.REPLACE, 0, false);
        doc.selection.fill(color(fill), ColorBlendMode.NORMAL, 100, false); doc.selection.deselect();
        layer.opacity = opacity; if (rotation !== 0) layer.rotate(rotation, AnchorPosition.MIDDLECENTER);
    }
    function removeLayer(id) {
        var layer = findLayer(doc, id); if (layer === null) throw new Error("LAYER_NOT_FOUND"); layer.remove();
    }
    function addPolygon(name, points, fill, opacity) {
        var layer = doc.artLayers.add(); layer.name = name;
        doc.selection.select(points, SelectionType.REPLACE, 0, false);
        doc.selection.fill(color(fill), ColorBlendMode.NORMAL, 100, false); doc.selection.deselect(); layer.opacity = opacity;
    }
    function addText(name, text, x, y, sizePt, fill, font, justification, tracking, rotation, opacity) {
        var layer = doc.artLayers.add(); layer.name = name; layer.kind = LayerKind.TEXT;
        var item = layer.textItem; item.kind = TextType.POINTTEXT; item.contents = text;
        item.position = [UnitValue(x, "px"), UnitValue(y, "px")]; item.size = UnitValue(sizePt, "pt");
        item.color = color(fill); item.tracking = tracking;
        if (font !== null) item.font = font;
        item.justification = justification === "center" ? Justification.CENTER : (justification === "right" ? Justification.RIGHT : Justification.LEFT);
        layer.opacity = opacity; if (rotation !== 0) layer.rotate(rotation, AnchorPosition.MIDDLECENTER);
    }
    try {
        app.activeDocument = doc;
        rollbackState = doc.activeHistoryState;
        app.preferences.rulerUnits = Units.PIXELS;
__OPERATIONS__
        for (var cleanupIndex = doc.layers.length - 1; cleanupIndex >= 0; cleanupIndex--) {
            if (doc.layers[cleanupIndex].name === "__TOOLKIT_EMPTY__") { doc.layers[cleanupIndex].remove(); break; }
        }
    } catch (operationError) {
        if (rollbackState !== null) {
            try { doc.activeHistoryState = rollbackState; }
            catch (rollbackError) { throw new Error("ROLLBACK_FAILED: " + rollbackError + "; ORIGINAL: " + operationError); }
        }
        throw operationError;
    } finally {
        app.preferences.rulerUnits = previousUnits;
        if (previousLayerId !== null) { var previousLayer = findLayer(doc, previousLayerId); if (previousLayer !== null) doc.activeLayer = previousLayer; }
        if (previousDocument !== null) app.activeDocument = previousDocument;
    }
    return "{\"id\":" + doc.id + ",\"name\":\"" + esc(doc.name) + "\",\"layerCount\":" + doc.layers.length + "}";
})()"#;

pub(super) const SAVE_SCRIPT: &str = r#"(function () {
    function esc(value) { return String(value).replace(/\\/g, "\\\\").replace(/\"/g, "\\\""); }
    function findDocument(id) { for (var i=0;i<app.documents.length;i++) if (app.documents[i].id===id) return app.documents[i]; throw new Error("STALE_SESSION"); }
    var doc = findDocument(__DOCUMENT_ID__); var previous = null; try { previous = app.activeDocument; } catch (_) {}
    try { app.activeDocument = doc; var options = new PhotoshopSaveOptions(); options.layers = true; doc.saveAs(new File(__PATH__), options, false, Extension.LOWERCASE); }
    finally { if (previous !== null && previous.id !== doc.id) app.activeDocument = previous; }
    return "{\"id\":" + doc.id + ",\"name\":\"" + esc(doc.name) + "\",\"path\":\"" + esc(__PATH__) + "\"}";
})()"#;

pub(super) const EXPORT_SCRIPT: &str = r#"(function () {
    function esc(value) { return String(value).replace(/\\/g, "\\\\").replace(/\"/g, "\\\""); }
    function findDocument(id) { for (var i=0;i<app.documents.length;i++) if (app.documents[i].id===id) return app.documents[i]; throw new Error("STALE_SESSION"); }
    var doc = findDocument(__DOCUMENT_ID__); var previous = null; var duplicate = null; try { previous = app.activeDocument; } catch (_) {}
    try {
        app.activeDocument = doc; duplicate = doc.duplicate(); duplicate.flatten();
        var options = new PNGSaveOptions(); options.interlaced = false;
        duplicate.saveAs(new File(__PATH__), options, true, Extension.LOWERCASE);
    } finally {
        if (duplicate !== null) duplicate.close(SaveOptions.DONOTSAVECHANGES);
        if (previous !== null) app.activeDocument = previous;
    }
    return "{\"id\":" + doc.id + ",\"path\":\"" + esc(__PATH__) + "\"}";
})()"#;

pub(super) const CLOSE_SCRIPT: &str = r#"(function () {
    function findDocument(id) { for (var i=0;i<app.documents.length;i++) if (app.documents[i].id===id) return app.documents[i]; throw new Error("STALE_SESSION"); }
    var doc = findDocument(__DOCUMENT_ID__); var name = doc.name; doc.close(SaveOptions.DONOTSAVECHANGES);
    return "{\"closed\":true,\"id\":" + __DOCUMENT_ID__ + ",\"name\":\"" + name.replace(/\\/g, "\\\\").replace(/\"/g, "\\\"") + "\"}";
})()"#;

pub(super) const INSPECT_SCRIPT: &str = r#"(function () {
    function esc(value) { return String(value).replace(/\\/g, "\\\\").replace(/\"/g, "\\\"").replace(/\r/g, "\\r").replace(/\n/g, "\\n"); }
    function q(value) { return "\"" + esc(value) + "\""; }
    function findDocument(id) { for (var i=0;i<app.documents.length;i++) if (app.documents[i].id===id) return app.documents[i]; throw new Error("STALE_SESSION"); }
    var doc = findDocument(__DOCUMENT_ID__); var items = []; var count = 0; var truncated = false;
    function walk(container, depth) {
        if (depth > __MAX_DEPTH__) return;
        for (var i=0;i<container.layers.length;i++) {
            if (count >= __MAX_ITEMS__) { truncated = true; return; }
            var layer = container.layers[i]; var kind = layer.typename; var text = null; var size = null;
            if (layer.typename === "ArtLayer") { try { kind = String(layer.kind); } catch (_) {} try { text = layer.textItem.contents; size = layer.textItem.size.as("pt"); } catch (_) {} }
            items.push("{\"id\":"+layer.id+",\"name\":"+q(layer.name)+",\"depth\":"+depth+",\"kind\":"+q(kind)+",\"visible\":"+(layer.visible?"true":"false")+",\"opacity\":"+layer.opacity+",\"text\":"+(text===null?"null":q(text))+",\"fontSizePt\":"+(size===null?"null":size)+"}");
            count++; if (layer.typename === "LayerSet") walk(layer, depth + 1); if (truncated) return;
        }
    }
    walk(doc, 0);
    return "{\"document\":{\"id\":"+doc.id+",\"name\":"+q(doc.name)+",\"width\":"+doc.width.as("px")+",\"height\":"+doc.height.as("px")+",\"resolution\":"+doc.resolution+"},\"layers\":["+items.join(",")+"],\"truncated\":"+(truncated?"true":"false")+"}";
})()"#;
