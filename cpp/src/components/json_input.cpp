#include "components/json_input.hpp"

#include <fstream>
#include <iostream>
#include <iterator>

namespace act::components {

JsonInputResult read_json_input(
    const std::string& source,
    const std::size_t maximum_bytes) {
    if (source.empty() || maximum_bytes == 0U) {
        return JsonInputResult{
            std::nullopt,
            "INVALID_ARGUMENT",
            "JSON input requires a source and a positive size limit.",
        };
    }
    std::ifstream file;
    std::istream* stream = nullptr;
    if (source == "-") {
        stream = &std::cin;
    } else {
        file.open(source, std::ios::binary);
        if (!file.is_open()) {
            return JsonInputResult{
                std::nullopt,
                "INPUT_READ_FAILED",
                "The explicit JSON input file could not be opened.",
            };
        }
        stream = &file;
    }
    std::string text;
    text.reserve(4096U);
    char buffer[4096]{};
    while (stream->good()) {
        stream->read(buffer, sizeof(buffer));
        const std::streamsize count = stream->gcount();
        if (count <= 0) {
            break;
        }
        if (text.size() +
                static_cast<std::size_t>(count) >
            maximum_bytes) {
            return JsonInputResult{
                std::nullopt,
                "INPUT_TOO_LARGE",
                "JSON input exceeds the bounded request size.",
            };
        }
        text.append(
            buffer, static_cast<std::size_t>(count));
    }
    if (stream->bad()) {
        return JsonInputResult{
            std::nullopt,
            "INPUT_READ_FAILED",
            "The explicit JSON input could not be read.",
        };
    }
    std::string parse_error;
    auto value = Json::parse(text, parse_error);
    if (!value.has_value()) {
        return JsonInputResult{
            std::nullopt,
            "INVALID_ARGUMENT",
            "The explicit input must contain one valid JSON value.",
        };
    }
    return JsonInputResult{
        std::move(value), {}, {}};
}

}  // namespace act::components
