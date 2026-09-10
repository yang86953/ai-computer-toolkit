#include "components/json.hpp"

#include <cmath>
#include <iostream>
#include <string>

namespace {

bool expect(
    const bool condition,
    const char* message) {
    if (!condition) {
        std::cerr << message << '\n';
        return false;
    }
    return true;
}

}  // namespace

int main() {
    bool ok = true;
    std::string error;
    const auto parsed = act::components::Json::parse(
        R"({"threshold":0.035,"small":5e-3,"count":8})",
        error);
    const auto* threshold =
        parsed.has_value() ? parsed->find("threshold") : nullptr;
    const auto* small =
        parsed.has_value() ? parsed->find("small") : nullptr;
    const auto* count =
        parsed.has_value() ? parsed->find("count") : nullptr;
    ok = expect(
             parsed.has_value() &&
                 threshold != nullptr &&
                 threshold->double_value() != nullptr &&
                 std::abs(
                     *threshold->double_value() - 0.035) <
                     1e-12 &&
                 small != nullptr &&
                 small->double_value() != nullptr &&
                 std::abs(*small->double_value() - 0.005) <
                     1e-12 &&
                 count != nullptr &&
                 count->integer_value() != nullptr &&
                 *count->integer_value() == 8,
             "Finite JSON numbers did not preserve numeric kinds.") &&
         ok;
    if (parsed.has_value()) {
        const auto round_trip =
            act::components::Json::parse(parsed->dump(), error);
        ok = expect(
                 round_trip.has_value() &&
                     round_trip->find("threshold") != nullptr &&
                     round_trip->find("threshold")
                             ->double_value() != nullptr,
                 "Finite JSON double did not round-trip.") &&
             ok;
    }
    for (const char* invalid :
         {"01", "-01", "1.", "1e", "1e+", "1e9999"}) {
        error.clear();
        ok = expect(
                 !act::components::Json::parse(
                      invalid, error).has_value(),
                 "Invalid or non-finite JSON number was accepted.") &&
             ok;
    }
    const act::components::Json whole_double(1.0);
    ok = expect(
             whole_double.dump() == "1.0",
             "A whole-valued double lost its numeric kind.") &&
         ok;
    return ok ? 0 : 1;
}
