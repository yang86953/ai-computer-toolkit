#include <windows.h>

#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Media.Core.h>
#include <winrt/Windows.Media.Playback.h>
#include <winrt/Windows.Storage.h>
#include <winrt/base.h>

#include <array>
#include <chrono>
#include <cstdint>
#include <fstream>
#include <filesystem>
#include <iostream>
#include <string>
#include <thread>

namespace {

void write_u16(
    std::ofstream& output,
    const std::uint16_t value) {
    const std::array<char, 2> bytes{
        static_cast<char>(value & 0xffU),
        static_cast<char>((value >> 8U) & 0xffU),
    };
    output.write(bytes.data(), bytes.size());
}

void write_u32(
    std::ofstream& output,
    const std::uint32_t value) {
    const std::array<char, 4> bytes{
        static_cast<char>(value & 0xffU),
        static_cast<char>((value >> 8U) & 0xffU),
        static_cast<char>((value >> 16U) & 0xffU),
        static_cast<char>((value >> 24U) & 0xffU),
    };
    output.write(bytes.data(), bytes.size());
}

bool write_silent_wav(const std::wstring& path) {
    constexpr std::uint32_t sample_rate = 44100U;
    constexpr std::uint32_t samples = sample_rate * 4U;
    constexpr std::uint32_t data_bytes = samples * 2U;
    std::ofstream output(
        std::filesystem::path(path),
        std::ios::binary | std::ios::trunc);
    if (!output) {
        return false;
    }
    output.write("RIFF", 4);
    write_u32(output, 36U + data_bytes);
    output.write("WAVEfmt ", 8);
    write_u32(output, 16U);
    write_u16(output, 1U);
    write_u16(output, 1U);
    write_u32(output, sample_rate);
    write_u32(output, sample_rate * 2U);
    write_u16(output, 2U);
    write_u16(output, 16U);
    output.write("data", 4);
    write_u32(output, data_bytes);
    const std::array<char, 1024> silence{};
    for (std::uint32_t offset = 0U;
         offset < data_bytes;
         offset += silence.size()) {
        const auto remaining = data_bytes - offset;
        output.write(
            silence.data(),
            static_cast<std::streamsize>(
                (remaining < silence.size())
                    ? remaining
                    : silence.size()));
    }
    return output.good();
}

}  // namespace

int wmain(int argc, wchar_t** argv) {
    if (argc != 3) {
        return 2;
    }
    const std::wstring wav_path = argv[1];
    const std::wstring ready_path = argv[2];
    if (!write_silent_wav(wav_path)) {
        return 3;
    }
    try {
        winrt::init_apartment(winrt::apartment_type::multi_threaded);
        using namespace winrt::Windows::Media::Core;
        using namespace winrt::Windows::Media::Playback;
        using namespace winrt::Windows::Storage;
        const auto file =
            StorageFile::GetFileFromPathAsync(wav_path).get();
        const auto first_source =
            MediaSource::CreateFromStorageFile(file);
        const auto second_source =
            MediaSource::CreateFromStorageFile(file);
        MediaPlaybackItem first(first_source);
        MediaPlaybackItem second(second_source);
        MediaPlaybackList playlist;
        playlist.Items().Append(first);
        playlist.Items().Append(second);
        playlist.AutoRepeatEnabled(true);
        MediaPlayer player;
        player.CommandManager().IsEnabled(true);
        player.Source(playlist);
        player.Volume(0.0);
        auto updater =
            player.SystemMediaTransportControls().DisplayUpdater();
        updater.Type(
            winrt::Windows::Media::MediaPlaybackType::Music);
        updater.MusicProperties().Title(
            L"ACT Media Control Fixture");
        updater.MusicProperties().Artist(
            L"ai-computer-toolkit");
        updater.Update();
        player.Play();
        {
            std::ofstream ready(
                std::filesystem::path(ready_path),
                std::ios::binary | std::ios::trunc);
            ready << "ready";
        }
        std::this_thread::sleep_for(std::chrono::seconds(45));
        player.Pause();
        return 0;
    } catch (const winrt::hresult_error& error) {
        std::wcerr << L"fixture WinRT error 0x" << std::hex
                   << static_cast<std::uint32_t>(error.code())
                   << L": " << error.message().c_str() << L'\n';
        return 4;
    }
}
