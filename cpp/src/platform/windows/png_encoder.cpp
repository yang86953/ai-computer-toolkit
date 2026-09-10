#include "platform/windows/png_encoder.hpp"

#include <windows.h>
#include <objidl.h>
#include <wincodec.h>

#include <winrt/base.h>

namespace act::platform::windows {
namespace {

constexpr std::uint32_t channels = 4U;
constexpr std::size_t maximum_pixel_bytes =
    64U * 1024U * 1024U;
constexpr std::uint64_t maximum_png_bytes =
    64ULL * 1024ULL * 1024ULL;

PngEncodeResult failure(
    const char* code,
    const char* message) {
    return PngEncodeResult{
        std::nullopt,
        code,
        message,
    };
}

}  // namespace

PngEncodeResult encode_rgba_png(
    const std::uint32_t width,
    const std::uint32_t height,
    const std::vector<std::uint8_t>& rgba) {
    if (width == 0U || height == 0U ||
        width > 4096U || height > 4096U) {
        return failure(
            "INVALID_ARGUMENT",
            "PNG encoding requires dimensions from 1 through 4096.");
    }
    const std::uint64_t expected =
        static_cast<std::uint64_t>(width) *
        static_cast<std::uint64_t>(height) * channels;
    if (expected > maximum_pixel_bytes ||
        expected != rgba.size()) {
        return failure(
            "RESOURCE_LIMIT_EXCEEDED",
            "RGBA input violates the certified PNG memory bounds.");
    }

    winrt::com_ptr<IWICImagingFactory> factory;
    HRESULT status = CoCreateInstance(
        CLSID_WICImagingFactory,
        nullptr,
        CLSCTX_INPROC_SERVER,
        IID_IWICImagingFactory,
        factory.put_void());
    if (FAILED(status)) {
        return failure(
            "ENCODER_UNAVAILABLE",
            "Windows Imaging Component is unavailable.");
    }

    winrt::com_ptr<IWICBitmap> bitmap;
    status = factory->CreateBitmapFromMemory(
        width,
        height,
        GUID_WICPixelFormat32bppRGBA,
        width * channels,
        static_cast<UINT>(rgba.size()),
        const_cast<BYTE*>(rgba.data()),
        bitmap.put());
    winrt::com_ptr<IWICFormatConverter> converter;
    if (SUCCEEDED(status)) {
        status = factory->CreateFormatConverter(converter.put());
    }
    BOOL can_convert = FALSE;
    if (SUCCEEDED(status)) {
        status = converter->CanConvert(
            GUID_WICPixelFormat32bppRGBA,
            GUID_WICPixelFormat32bppBGRA,
            &can_convert);
    }
    if (SUCCEEDED(status) && can_convert == FALSE) {
        return failure(
            "ENCODER_UNAVAILABLE",
            "WIC cannot convert certified RGBA8 input for PNG encoding.");
    }
    if (SUCCEEDED(status)) {
        status = converter->Initialize(
            bitmap.get(),
            GUID_WICPixelFormat32bppBGRA,
            WICBitmapDitherTypeNone,
            nullptr,
            0.0,
            WICBitmapPaletteTypeCustom);
    }
    if (FAILED(status)) {
        return failure(
            "ENCODER_UNAVAILABLE",
            "WIC could not prepare the certified RGBA8 source.");
    }

    winrt::com_ptr<IStream> stream;
    status = CreateStreamOnHGlobal(
        nullptr, TRUE, stream.put());
    if (FAILED(status)) {
        return failure(
            "OPERATION_FAILED",
            "The in-memory PNG stream could not be created.");
    }
    winrt::com_ptr<IWICBitmapEncoder> encoder;
    status = factory->CreateEncoder(
        GUID_ContainerFormatPng, nullptr, encoder.put());
    if (SUCCEEDED(status)) {
        status = encoder->Initialize(
            stream.get(), WICBitmapEncoderNoCache);
    }

    winrt::com_ptr<IWICBitmapFrameEncode> frame;
    winrt::com_ptr<IPropertyBag2> properties;
    if (SUCCEEDED(status)) {
        status = encoder->CreateNewFrame(
            frame.put(), properties.put());
    }
    if (SUCCEEDED(status)) {
        status = frame->Initialize(properties.get());
    }
    if (SUCCEEDED(status)) {
        status = frame->SetSize(width, height);
    }
    WICPixelFormatGUID format = GUID_WICPixelFormat32bppBGRA;
    if (SUCCEEDED(status)) {
        status = frame->SetPixelFormat(&format);
    }
    if (SUCCEEDED(status) &&
        !IsEqualGUID(format, GUID_WICPixelFormat32bppBGRA)) {
        return failure(
            "ENCODER_UNAVAILABLE",
            "WIC cannot encode the certified converted pixel format.");
    }
    if (SUCCEEDED(status)) {
        status = frame->WriteSource(converter.get(), nullptr);
    }
    if (SUCCEEDED(status)) {
        status = frame->Commit();
    }
    if (SUCCEEDED(status)) {
        status = encoder->Commit();
    }
    if (FAILED(status)) {
        return failure(
            "OPERATION_FAILED",
            "Windows Imaging Component failed to encode PNG in memory.");
    }

    STATSTG statistics{};
    status = stream->Stat(&statistics, STATFLAG_NONAME);
    const std::uint64_t size =
        static_cast<std::uint64_t>(statistics.cbSize.QuadPart);
    if (FAILED(status) ||
        size == 0U ||
        size > maximum_png_bytes) {
        return failure(
            "RESOURCE_LIMIT_EXCEEDED",
            "The encoded PNG violates the certified memory limit.");
    }
    LARGE_INTEGER beginning{};
    status = stream->Seek(beginning, STREAM_SEEK_SET, nullptr);
    PngMemoryImage image{
        std::vector<std::uint8_t>(
            static_cast<std::size_t>(size)),
    };
    ULONG read = 0U;
    if (SUCCEEDED(status)) {
        status = stream->Read(
            image.bytes.data(),
            static_cast<ULONG>(image.bytes.size()),
            &read);
    }
    if (FAILED(status) || read != image.bytes.size()) {
        return failure(
            "OPERATION_FAILED",
            "The encoded PNG could not be read from memory.");
    }
    return PngEncodeResult{
        std::move(image),
        {},
        {},
    };
}

}  // namespace act::platform::windows
