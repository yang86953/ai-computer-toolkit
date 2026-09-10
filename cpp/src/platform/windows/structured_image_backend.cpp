#include "platform/windows/structured_image_backend.hpp"

#include "components/json.hpp"
#include "components/opaque_id.hpp"
// 复用共享的 opaque 目标唯一匹配组件。
#include "components/opaque_target_match.hpp"
#include "components/output_path_policy.hpp"
#include "platform/windows/process_backend.hpp"
#include "platform/windows/text_codec.hpp"

#include <windows.h>
#include <oleauto.h>

#include <array>
#include <iomanip>
#include <sstream>
#include <string_view>

namespace act::platform::windows {

// 从稳定 provider key 与进程代际事实生成应用会话。
std::string structured_image_application_session_id(
    // 接收当前 Photoshop PID。
    const std::uint32_t process_id,
    // 接收同一进程实例的创建 FILETIME。
    const std::uint64_t creation_time) {
    // 使用与 Rust 相同的 provider-key:PID:FILETIME 字节布局。
    const std::string identity =
        // 固定 provider key，避免公开 COM ProgID。
        "structured-image-editor:" +
        // 拼接十进制 PID。
        std::to_string(process_id) + ':' +
        // 拼接十进制进程创建 FILETIME。
        std::to_string(creation_time);
    // 仅返回 canonical s2:a 指纹。
    return components::opaque_id(
        // 使用应用目标 kind。
        'a',
        // 传入 provider 私有身份字节。
        identity);
// 结束应用会话生成。
}

// 从稳定进程代际与文档事实生成文档会话。
std::string structured_image_document_session_id(
    // 接收当前 Photoshop PID。
    const std::uint32_t process_id,
    // 接收同一进程实例的创建 FILETIME。
    const std::uint64_t creation_time,
    // 接收 provider 原生文档 ID。
    const std::int64_t native_document_id,
    // 接收当前文档名称。
    const std::string& name,
    // 接收当前源路径或空字符串哨兵。
    const std::string& source_path) {
    // 使用与 Rust 相同的 PID/NUL/FILETIME/NUL/document facts 布局。
    const std::string identity =
        // 拼接十进制 PID。
        std::to_string(process_id) + '\0' +
        // 拼接十进制创建 FILETIME。
        std::to_string(creation_time) + '\0' +
        // 拼接原生文档 ID。
        std::to_string(native_document_id) + '\0' +
        // 拼接当前文档名称。
        name + '\0' +
        // 拼接当前源路径或空字符串哨兵。
        source_path;
    // 仅返回 canonical s2:d 指纹。
    return components::opaque_id(
        // 使用文档目标 kind。
        'd',
        // 传入 provider 私有身份字节。
        identity);
// 结束文档会话生成。
}

namespace {

constexpr std::wstring_view provider_prog_id =
    L"Photoshop.Application";
constexpr HRESULT active_object_unavailable =
    static_cast<HRESULT>(0x800401E3L);

const char* documents_script = R"JS((function () {
function esc(v){return String(v).replace(/\\/g,"\\\\").replace(/"/g,"\\\"").replace(/\r/g,"\\r").replace(/\n/g,"\\n").replace(/\t/g,"\\t");}
function q(v){return "\""+esc(v)+"\"";} var items=[];
for(var i=0;i<app.documents.length;i++){var d=app.documents[i],p=null;try{p=d.fullName.fsName;}catch(_){}
items.push("{\"id\":"+d.id+",\"name\":"+q(d.name)+",\"path\":"+(p===null?"null":q(p))+",\"width\":"+d.width.as("px")+",\"height\":"+d.height.as("px")+",\"resolution\":"+d.resolution+",\"layerCount\":"+d.layers.length+",\"saved\":"+(d.saved?"true":"false")+",\"active\":"+(app.activeDocument.id===d.id?"true":"false")+"}");}
return "{\"documents\":["+items.join(",")+"]}";})())JS";

const char* save_script = R"JS((function () {
function esc(v){return String(v).replace(/\\/g,"\\\\").replace(/"/g,"\\\"");}
function findDocument(id){for(var i=0;i<app.documents.length;i++)if(app.documents[i].id===id)return app.documents[i];throw new Error("STALE_SESSION");}
var doc=findDocument(__DOCUMENT_ID__),previous=null;try{previous=app.activeDocument;}catch(_){}
try{app.activeDocument=doc;var options=new PhotoshopSaveOptions();options.layers=true;doc.saveAs(new File(__PATH__),options,false,Extension.LOWERCASE);}
finally{if(previous!==null&&previous.id!==doc.id)app.activeDocument=previous;}
return "{\"id\":"+doc.id+",\"name\":\""+esc(doc.name)+"\",\"path\":\""+esc(__PATH__)+"\"}";})())JS";

const char* export_script = R"JS((function () {
function esc(v){return String(v).replace(/\\/g,"\\\\").replace(/"/g,"\\\"");}
function findDocument(id){for(var i=0;i<app.documents.length;i++)if(app.documents[i].id===id)return app.documents[i];throw new Error("STALE_SESSION");}
var doc=findDocument(__DOCUMENT_ID__),previous=null,duplicate=null;try{previous=app.activeDocument;}catch(_){}
try{app.activeDocument=doc;duplicate=doc.duplicate();duplicate.flatten();var options=new PNGSaveOptions();options.interlaced=false;duplicate.saveAs(new File(__PATH__),options,true,Extension.LOWERCASE);}
finally{if(duplicate!==null)duplicate.close(SaveOptions.DONOTSAVECHANGES);if(previous!==null)app.activeDocument=previous;}
return "{\"id\":"+doc.id+",\"path\":\""+esc(__PATH__)+"\"}";})())JS";

std::string hresult_text(const HRESULT result) {
    std::ostringstream output;
    output << "HRESULT 0x" << std::hex << std::setw(8)
           << std::setfill('0')
           << static_cast<std::uint32_t>(result);
    return output.str();
}

class ComApartment final {
public:
    ComApartment() {
        result_ = CoInitializeEx(
            nullptr, COINIT_APARTMENTTHREADED);
        initialized_ = SUCCEEDED(result_);
    }
    ~ComApartment() {
        if (initialized_) {
            CoUninitialize();
        }
    }
    ComApartment(const ComApartment&) = delete;
    ComApartment& operator=(const ComApartment&) = delete;
    [[nodiscard]] HRESULT result() const {
        return result_;
    }

private:
    HRESULT result_ = E_FAIL;
    bool initialized_ = false;
};

struct ScriptResult {
    bool ok;
    bool invoked;
    std::string text;
    std::string error;
};

class DispatchConnection final {
public:
    explicit DispatchConnection(IDispatch* dispatch)
        : dispatch_(dispatch) {}
    ~DispatchConnection() {
        if (dispatch_ != nullptr) {
            dispatch_->Release();
        }
    }
    DispatchConnection(const DispatchConnection&) = delete;
    DispatchConnection& operator=(const DispatchConnection&) = delete;

    [[nodiscard]] ScriptResult run(
        const std::string_view script) const {
        OLECHAR member[] = L"DoJavaScript";
        LPOLESTR names[] = {member};
        DISPID identifier = 0;
        HRESULT result = dispatch_->GetIDsOfNames(
            IID_NULL,
            names,
            1,
            LOCALE_USER_DEFAULT,
            &identifier);
        if (FAILED(result)) {
            return ScriptResult{
                false, false, {}, hresult_text(result)};
        }
        const std::wstring wide_script = wide(script);
        BSTR argument_text = SysAllocStringLen(
            wide_script.data(),
            static_cast<UINT>(wide_script.size()));
        if (argument_text == nullptr) {
            return ScriptResult{
                false, false, {}, "Could not allocate COM script input."};
        }
        VARIANTARG argument;
        VariantInit(&argument);
        V_VT(&argument) = VT_BSTR;
        V_BSTR(&argument) = argument_text;
        DISPPARAMS parameters{};
        parameters.rgvarg = &argument;
        parameters.cArgs = 1;
        VARIANT value;
        VariantInit(&value);
        EXCEPINFO exception{};
        UINT argument_error = 0;
        result = dispatch_->Invoke(
            identifier,
            IID_NULL,
            LOCALE_USER_DEFAULT,
            DISPATCH_METHOD,
            &parameters,
            &value,
            &exception,
            &argument_error);
        VariantClear(&argument);
        if (FAILED(result)) {
            const std::string error = exception.bstrDescription != nullptr
                ? utf8(exception.bstrDescription)
                : hresult_text(result);
            SysFreeString(exception.bstrSource);
            SysFreeString(exception.bstrDescription);
            SysFreeString(exception.bstrHelpFile);
            VariantClear(&value);
            return ScriptResult{false, true, {}, error};
        }
        if (V_VT(&value) != VT_BSTR ||
            V_BSTR(&value) == nullptr) {
            VariantClear(&value);
            return ScriptResult{
                false,
                true,
                {},
                "Provider returned a non-text script result.",
            };
        }
        const std::string text = utf8(
            std::wstring_view(
                V_BSTR(&value),
                SysStringLen(V_BSTR(&value))));
        VariantClear(&value);
        return ScriptResult{true, true, text, {}};
    }

private:
    IDispatch* dispatch_;
};

struct AttachResult {
    std::optional<DispatchConnection*> connection;
    std::optional<std::string> error_code;
    std::optional<std::string> error_message;
};

AttachResult attach(ComApartment& apartment) {
    if (FAILED(apartment.result())) {
        return AttachResult{
            std::nullopt,
            "COM_INITIALIZATION_FAILED",
            hresult_text(apartment.result()),
        };
    }
    CLSID class_id{};
    const std::wstring prog_id(provider_prog_id);
    HRESULT result = CLSIDFromProgID(
        prog_id.c_str(), &class_id);
    if (FAILED(result)) {
        return AttachResult{
            std::nullopt,
            "APPLICATION_NOT_INSTALLED",
            hresult_text(result),
        };
    }
    IUnknown* unknown = nullptr;
    result = GetActiveObject(class_id, nullptr, &unknown);
    if (result == active_object_unavailable) {
        return AttachResult{std::nullopt, std::nullopt, std::nullopt};
    }
    if (FAILED(result) || unknown == nullptr) {
        return AttachResult{
            std::nullopt,
            "OPERATION_FAILED",
            hresult_text(result),
        };
    }
    IDispatch* dispatch = nullptr;
    result = unknown->QueryInterface(
        IID_IDispatch,
        reinterpret_cast<void**>(&dispatch));
    unknown->Release();
    if (FAILED(result) || dispatch == nullptr) {
        return AttachResult{
            std::nullopt,
            "OPERATION_FAILED",
            hresult_text(result),
        };
    }
    return AttachResult{
        new DispatchConnection(dispatch),
        std::nullopt,
        std::nullopt,
    };
}

// 保存 structured-image provider 的私有进程代际事实。
struct ProviderProcessIdentity {
    // 保存原生 PID，仅用于重新发现。
    std::uint32_t process_id;
    // 保存进程创建 FILETIME，抵抗 PID 复用。
    std::uint64_t creation_time;
    // 结束 provider 进程代际事定义。
};

// 枚举当前 structured-image provider 进程实例。
std::vector<ProviderProcessIdentity> provider_process_ids() {
    const auto inventory =
        ProcessBackend().enumerate_processes(16384U);
    // 收集 PID 与创建时间组成的精确进程实例。
    std::vector<ProviderProcessIdentity> matches;
    for (const auto& process : inventory.records) {
        if (process.process_name == "Photoshop.exe" ||
            process.process_name == "photoshop.exe") {
            // 保持两个私有事实来自同一 ProcessBackend 快照。
            matches.push_back(ProviderProcessIdentity{
                // 保存原生 PID。
                process.native_process_id,
                // 保存同一快照的进程创建 FILETIME。
                process.native_creation_time,
            });
        }
    }
    return matches;
}

// 使用绑定进程代际的公用纯身份生成器。
std::string application_id(const ProviderProcessIdentity& process) {
    // 传入同一快照的 PID 与创建 FILETIME。
    return structured_image_application_session_id(
        // 传入 provider 私有 PID。
        process.process_id,
        // 传入 provider 私有创建 FILETIME。
        process.creation_time);
}

std::optional<StructuredImageDocumentRecord> parse_document(
    const components::Json& item,
    // 接收同一 inventory 的进程代际事实。
    const ProviderProcessIdentity& process) {
    const auto* id = item.find("id");
    const auto* name = item.find("name");
    const auto* width = item.find("width");
    const auto* height = item.find("height");
    const auto* resolution = item.find("resolution");
    const auto* layers = item.find("layerCount");
    const auto* saved = item.find("saved");
    const auto* active = item.find("active");
    if (id == nullptr || id->integer_value() == nullptr ||
        name == nullptr || name->string_value() == nullptr ||
        width == nullptr || height == nullptr ||
        resolution == nullptr || layers == nullptr ||
        layers->integer_value() == nullptr ||
        saved == nullptr || saved->bool_value() == nullptr ||
        active == nullptr || active->bool_value() == nullptr) {
        return std::nullopt;
    }
    const auto number = [](const components::Json* value)
        -> std::optional<double> {
        if (value->double_value() != nullptr) {
            return *value->double_value();
        }
        if (value->integer_value() != nullptr) {
            return static_cast<double>(*value->integer_value());
        }
        return std::nullopt;
    };
    const auto width_value = number(width);
    const auto height_value = number(height);
    const auto resolution_value = number(resolution);
    if (!width_value.has_value() ||
        !height_value.has_value() ||
        !resolution_value.has_value()) {
        return std::nullopt;
    }
    std::optional<std::string> path;
    const auto* source_path = item.find("path");
    if (source_path != nullptr &&
        source_path->string_value() != nullptr) {
        path = *source_path->string_value();
    }
    const std::int64_t native_id = *id->integer_value();
    // 调用可独立验证的纯文档身份生成器。
    const std::string session_id = structured_image_document_session_id(
        // 传入同一快照的 PID。
        process.process_id,
        // 传入同一快照的创建 FILETIME。
        process.creation_time,
        // 传入 provider 原生文档 ID。
        native_id,
        // 传入当前文档名称。
        *name->string_value(),
        // 传入当前源路径或空字符串哨兵。
        path.value_or(""));
    return StructuredImageDocumentRecord{
        session_id,
        *name->string_value(),
        std::move(path),
        *width_value,
        *height_value,
        *resolution_value,
        *layers->integer_value(),
        *saved->bool_value(),
        *active->bool_value(),
        process.process_id,
        native_id,
    };
}

StructuredImageInventory inventory_with_connection(
    DispatchConnection& connection) {
    const auto processes = provider_process_ids();
    if (processes.empty()) {
        return StructuredImageInventory{
            {}, std::nullopt, "APPLICATION_NOT_RUNNING",
            "No running structured image application is available.",
        };
    }
    if (processes.size() != 1U) {
        return StructuredImageInventory{
            {}, std::nullopt, "AMBIGUOUS_TARGET",
            "More than one provider process is running.",
        };
    }
    const ScriptResult script = connection.run(documents_script);
    if (!script.ok) {
        return StructuredImageInventory{
            {},
            std::nullopt,
            "OPERATION_FAILED",
            script.error,
        };
    }
    std::string parse_error;
    const auto value =
        components::Json::parse(script.text, parse_error);
    const auto* documents = value.has_value()
        ? value->find("documents")
        : nullptr;
    if (documents == nullptr ||
        documents->array_items() == nullptr) {
        return StructuredImageInventory{
            {},
            std::nullopt,
            "OPERATION_FAILED",
            "Provider document inventory was invalid.",
        };
    }
    StructuredImageInventory inventory{
        {}, application_id(processes.front()),
        std::nullopt, std::nullopt};
    for (const auto& item : *documents->array_items()) {
        auto document =
            parse_document(item, processes.front());
        if (!document.has_value()) {
            return StructuredImageInventory{
                {},
                std::nullopt,
                "OPERATION_FAILED",
                "Provider document record was invalid.",
            };
        }
        inventory.documents.push_back(
            std::move(*document));
    }
    return inventory;
}

std::string replace_all(
    std::string source,
    const std::string_view needle,
    const std::string_view replacement) {
    std::size_t offset = 0U;
    while ((offset = source.find(needle, offset)) !=
           std::string::npos) {
        source.replace(offset, needle.size(), replacement);
        offset += replacement.size();
    }
    return source;
}

// 在当前 provider 文档清单中保留零、一或多命中状态。
auto resolve_document(
    const StructuredImageInventory& inventory,
    const std::string_view session_id) {
    // 完整扫描文档清单以拒绝 sessionId 碰撞。
    return components::match_opaque_target(
        inventory.documents.begin(),
        inventory.documents.end(),
        [session_id](const auto& document) {
            return document.session_id == session_id;
        });
}

StructuredImageWriteEvidence run_write(
    const StructuredImageDocumentRecord& requested,
    const std::filesystem::path& path,
    const std::string_view script_template,
    const bool verify_saved_state) {
    ComApartment apartment;
    AttachResult attached = attach(apartment);
    if (!attached.connection.has_value()) {
        return StructuredImageWriteEvidence{
            false,
            false,
            false,
            std::nullopt,
            attached.error_code.value_or("APPLICATION_NOT_RUNNING"),
            attached.error_message.value_or(
                "No running structured image application is available."),
        };
    }
    DispatchConnection* connection = *attached.connection;
    const StructuredImageInventory before =
        inventory_with_connection(*connection);
    const auto current =
        resolve_document(before, requested.session_id);
    // 区分 provider 错误、歧义和过期目标。
    if (before.error_code.has_value() ||
        current.state != components::OpaqueTargetMatchState::unique ||
        current.position->native_process_id !=
            requested.native_process_id ||
        current.position->native_document_id !=
            requested.native_document_id) {
        delete connection;
        // 多命中在写入前必须返回稳定歧义错误。
        const bool ambiguous = current.state ==
            components::OpaqueTargetMatchState::ambiguous;
        return StructuredImageWriteEvidence{
            false,
            false,
            false,
            std::nullopt,
            before.error_code.value_or(
                ambiguous ? "AMBIGUOUS_TARGET" : "STALE_SESSION"),
            before.error_message.value_or(
                ambiguous
                    ? "The image document session resolves to multiple documents."
                    : "The image document session is stale or changed."),
        };
    }
    // 唯一命中后才提取 provider 私有文档 ID。
    const auto& current_document = *current.position;
    std::string script(script_template);
    script = replace_all(
        std::move(script),
        "__DOCUMENT_ID__",
        std::to_string(current_document.native_document_id));
    script = replace_all(
        std::move(script),
        "__PATH__",
        components::Json(path.string()).dump());
    const ScriptResult result = connection->run(script);
    if (!result.ok) {
        delete connection;
        return StructuredImageWriteEvidence{
            result.invoked,
            false,
            result.invoked,
            std::nullopt,
            result.invoked
                ? "STRUCTURED_IMAGE_OUTCOME_UNKNOWN"
                : "OPERATION_FAILED",
            result.error,
        };
    }
    if (!components::is_non_empty_regular_file(path)) {
        delete connection;
        return StructuredImageWriteEvidence{
            true,
            false,
            true,
            std::nullopt,
            "OUTPUT_VERIFICATION_FAILED",
            "The provider returned success but output is empty or absent.",
        };
    }
    const StructuredImageInventory after =
        inventory_with_connection(*connection);
    delete connection;
    const auto verified =
        resolve_document(after, requested.session_id);
    // 写入后的 provider 清单错误表示结果不可安全确认。
    if (after.error_code.has_value()) {
        // 保留 provider 的稳定错误与已派发事实。
        return StructuredImageWriteEvidence{
            true,
            false,
            true,
            std::nullopt,
            after.error_code,
            after.error_message.value_or(
                "The written image document could not be rediscovered."),
        };
    }
    // 写入后出现多命中时结果不可安全重试。
    if (verified.state == components::OpaqueTargetMatchState::ambiguous) {
        // 保留已派发与结果未知证据。
        return StructuredImageWriteEvidence{
            true,
            false,
            true,
            std::nullopt,
            "AMBIGUOUS_TARGET",
            "The written image document now resolves to multiple documents.",
        };
    }
    if (verify_saved_state) {
        if (after.error_code.has_value() ||
            verified.state != components::OpaqueTargetMatchState::unique ||
            !verified.position->saved ||
            !verified.position->source_path.has_value()) {
            return StructuredImageWriteEvidence{
                true,
                false,
                true,
                std::nullopt,
                "OUTPUT_VERIFICATION_FAILED",
                "The saved document state could not be verified.",
            };
        }
        std::error_code path_error;
        const auto expected =
            std::filesystem::weakly_canonical(path, path_error);
        const auto actual = path_error
            ? std::filesystem::path{}
            : std::filesystem::weakly_canonical(
                  *verified.position->source_path, path_error);
        if (path_error || expected != actual) {
            return StructuredImageWriteEvidence{
                true,
                false,
                true,
                std::nullopt,
                "OUTPUT_VERIFICATION_FAILED",
                "The provider saved a different source path.",
            };
        }
    }
    return StructuredImageWriteEvidence{
        true,
        true,
        false,
        verified.state == components::OpaqueTargetMatchState::unique
            ? std::optional(verified.position->session_id)
            : std::nullopt,
        std::nullopt,
        std::nullopt,
    };
}

}  // namespace

StructuredImageStatus StructuredImageBackend::status() const {
    ComApartment apartment;
    AttachResult attached = attach(apartment);
    if (attached.error_code == "APPLICATION_NOT_INSTALLED") {
        return StructuredImageStatus{
            false, false, std::nullopt,
            std::nullopt, std::nullopt};
    }
    if (attached.error_code.has_value()) {
        return StructuredImageStatus{
            true, false, std::nullopt,
            attached.error_code, attached.error_message};
    }
    if (!attached.connection.has_value()) {
        return StructuredImageStatus{
            true, false, std::nullopt,
            std::nullopt, std::nullopt};
    }
    DispatchConnection* connection = *attached.connection;
    const ScriptResult version =
        connection->run("app.version;");
    delete connection;
    if (!version.ok) {
        return StructuredImageStatus{
            true,
            true,
            std::nullopt,
            "OPERATION_FAILED",
            version.error,
        };
    }
    return StructuredImageStatus{
        true, true, version.text,
        std::nullopt, std::nullopt};
}

StructuredImageInventory
StructuredImageBackend::inventory() const {
    ComApartment apartment;
    AttachResult attached = attach(apartment);
    if (attached.error_code.has_value()) {
        return StructuredImageInventory{
            {},
            std::nullopt,
            attached.error_code,
            attached.error_message,
        };
    }
    if (!attached.connection.has_value()) {
        return StructuredImageInventory{
            {}, std::nullopt, std::nullopt, std::nullopt};
    }
    DispatchConnection* connection = *attached.connection;
    StructuredImageInventory result =
        inventory_with_connection(*connection);
    delete connection;
    return result;
}

StructuredImageWriteEvidence StructuredImageBackend::save(
    const StructuredImageDocumentRecord& document,
    const std::filesystem::path& path) const {
    return run_write(
        document, path, save_script, true);
}

StructuredImageWriteEvidence StructuredImageBackend::export_png(
    const StructuredImageDocumentRecord& document,
    const std::filesystem::path& path) const {
    return run_write(
        document, path, export_script, false);
}

}  // namespace act::platform::windows
