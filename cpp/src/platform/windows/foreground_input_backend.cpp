#include "platform/windows/foreground_input_backend.hpp"

#include <windows.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <thread>
#include <vector>

namespace act::platform::windows {
namespace {

WORD virtual_key(const components::NamedKey key, const char ascii) {
    switch (key) {
        case components::NamedKey::ascii:
            return static_cast<WORD>(
                static_cast<unsigned char>(ascii));
        case components::NamedKey::enter:
            return VK_RETURN;
        case components::NamedKey::tab:
            return VK_TAB;
        case components::NamedKey::escape:
            return VK_ESCAPE;
        case components::NamedKey::backspace:
            return VK_BACK;
        case components::NamedKey::delete_key:
            return VK_DELETE;
        case components::NamedKey::space:
            return VK_SPACE;
        case components::NamedKey::left:
            return VK_LEFT;
        case components::NamedKey::up:
            return VK_UP;
        case components::NamedKey::right:
            return VK_RIGHT;
        case components::NamedKey::down:
            return VK_DOWN;
        case components::NamedKey::home:
            return VK_HOME;
        case components::NamedKey::end:
            return VK_END;
        case components::NamedKey::f4:
            return VK_F4;
    }
    return 0U;
}

WORD virtual_modifier(
    const components::KeyModifier modifier) {
    switch (modifier) {
        case components::KeyModifier::control:
            return VK_CONTROL;
        case components::KeyModifier::alt:
            return VK_MENU;
        case components::KeyModifier::shift:
            return VK_SHIFT;
    }
    return 0U;
}

INPUT keyboard_input(const WORD key, const bool release) {
    INPUT input{};
    input.type = INPUT_KEYBOARD;
    input.ki.wVk = key;
    input.ki.dwFlags = release ? KEYEVENTF_KEYUP : 0U;
    return input;
}

std::vector<INPUT> inputs(
    const components::KeyChord& chord) {
    std::vector<WORD> keys;
    keys.reserve(chord.modifiers.size() + 1U);
    for (const auto modifier : chord.modifiers) {
        keys.push_back(virtual_modifier(modifier));
    }
    keys.push_back(virtual_key(
        chord.key, chord.ascii_character));
    std::vector<INPUT> result;
    result.reserve(keys.size() * 2U);
    for (const auto key : keys) {
        result.push_back(keyboard_input(key, false));
    }
    for (auto iterator = keys.rbegin();
         iterator != keys.rend();
         ++iterator) {
        result.push_back(keyboard_input(*iterator, true));
    }
    return result;
}

bool exact_window(
    const WindowRecord& window,
    HWND& native) {
    native = reinterpret_cast<HWND>(window.native_window);
    if (IsWindow(native) == FALSE) {
        return false;
    }
    DWORD process_id = 0U;
    GetWindowThreadProcessId(native, &process_id);
    return process_id == window.native_process_id;
}

bool acquire_foreground(const HWND target) {
    if (IsWindowVisible(target) == FALSE) {
        ShowWindowAsync(target, SW_RESTORE);
        std::this_thread::sleep_for(
            std::chrono::milliseconds(80));
    }
    for (int attempt = 0; attempt < 5; ++attempt) {
        if (GetForegroundWindow() == target) {
            return true;
        }
        SetForegroundWindow(target);
        if (GetForegroundWindow() == target) {
            return true;
        }
        std::this_thread::sleep_for(
            std::chrono::milliseconds(50));
    }
    return false;
}

}  // namespace

ForegroundKeyResult ForegroundInputBackend::press_key(
    const WindowRecord& window,
    const components::KeyChord& chord) const {
    HWND target = nullptr;
    if (!exact_window(window, target)) {
        return ForegroundKeyResult{
            std::nullopt,
            BackendError{
                "STALE_SESSION",
                "The exact foreground-input target no longer resolves.",
            },
        };
    }
    if (!acquire_foreground(target)) {
        return ForegroundKeyResult{
            std::nullopt,
            BackendError{
                "FOREGROUND_ACTIVATION_FAILED",
                "Windows did not grant foreground activation; no input "
                "was dispatched.",
            },
        };
    }
    const auto sequence = inputs(chord);
    const UINT sent = SendInput(
        static_cast<UINT>(sequence.size()),
        const_cast<INPUT*>(sequence.data()),
        static_cast<int>(sizeof(INPUT)));
    if (sent != sequence.size()) {
        return ForegroundKeyResult{
            std::nullopt,
            BackendError{
                "INPUT_OUTCOME_UNKNOWN",
                "Windows did not confirm the complete key sequence; "
                "automatic retry is unsafe.",
            },
        };
    }
    return ForegroundKeyResult{
        ForegroundKeyEvidence{
            true,
            true,
            GetForegroundWindow() == target,
        },
        std::nullopt,
    };
}

}  // namespace act::platform::windows
