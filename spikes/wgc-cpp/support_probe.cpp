#include <winrt/Windows.Graphics.Capture.h>
#include <winrt/base.h>

#include <iostream>

int main() {
    winrt::init_apartment(winrt::apartment_type::multi_threaded);
    const bool supported =
        winrt::Windows::Graphics::Capture::
            GraphicsCaptureSession::IsSupported();
    std::cout << (supported ? "supported\n" : "unsupported\n");
    return supported ? 0 : 2;
}
