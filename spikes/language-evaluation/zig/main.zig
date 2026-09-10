const std = @import("std");
const Io = std.Io;

const c = @cImport({
    @cDefine("COBJMACROS", "1");
    @cDefine("WIN32_LEAN_AND_MEAN", "1");
    @cInclude("windows.h");
    @cInclude("tlhelp32.h");
    @cInclude("uiautomation.h");
});

var discovered_window_count: u32 = 0;

fn countWindow(_: c.HWND, _: c.LPARAM) callconv(.winapi) c.BOOL {
    discovered_window_count += 1;
    return c.TRUE;
}

fn processCount() u32 {
    const snapshot = c.CreateToolhelp32Snapshot(c.TH32CS_SNAPPROCESS, 0);
    if (snapshot == c.INVALID_HANDLE_VALUE) return 0;
    defer _ = c.CloseHandle(snapshot);

    var entry: c.PROCESSENTRY32W = std.mem.zeroes(c.PROCESSENTRY32W);
    entry.dwSize = @sizeOf(c.PROCESSENTRY32W);
    var count: u32 = 0;
    if (c.Process32FirstW(snapshot, &entry) != 0) {
        while (true) {
            count += 1;
            if (c.Process32NextW(snapshot, &entry) == 0) break;
        }
    }
    return count;
}

fn windowCount() u32 {
    discovered_window_count = 0;
    if (c.EnumWindows(countWindow, 0) == 0) return 0;
    return discovered_window_count;
}

fn uiaAvailable() bool {
    const apartment = c.CoInitializeEx(null, c.COINIT_MULTITHREADED);
    if (apartment < 0) return false;
    defer c.CoUninitialize();

    var automation: ?*anyopaque = null;
    const result = c.CoCreateInstance(
        &c.CLSID_CUIAutomation,
        null,
        c.CLSCTX_INPROC_SERVER,
        &c.IID_IUIAutomation,
        &automation,
    );
    if (automation) |value| {
        const unknown: *c.IUnknown = @ptrCast(@alignCast(value));
        _ = unknown.lpVtbl.*.Release.?(unknown);
    }
    return result >= 0;
}

pub fn main(init: std.process.Init) !void {
    var stdout_buffer: [1024]u8 = undefined;
    var stdout_writer = Io.File.stdout().writer(init.io, &stdout_buffer);
    const stdout = &stdout_writer.interface;

    const foreground_before = c.GetForegroundWindow();
    const processes = processCount();
    const windows = windowCount();
    const uia_initialized = uiaAvailable();
    const foreground_after = c.GetForegroundWindow();
    const foreground_unchanged = foreground_before == foreground_after;
    const ok = processes > 0 and windows > 0 and uia_initialized and foreground_unchanged;

    try stdout.print(
        "{{\"ok\":{},\"contractVersion\":\"act/language-probe/v1\",\"implementationLanguage\":\"zig\",\"platform\":\"windows\",\"observations\":{{\"processCount\":{},\"topLevelWindowCount\":{},\"uiaClientInitialized\":{},\"foregroundUnchanged\":{}}},\"safety\":{{\"readOnly\":true,\"inputSent\":false,\"windowActivated\":false}}}}\n",
        .{ ok, processes, windows, uia_initialized, foreground_unchanged },
    );
    try stdout.flush();
    if (!ok) std.process.exit(2);
}
