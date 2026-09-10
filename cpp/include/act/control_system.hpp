#pragma once

#include <memory>
#include <string>
#include <vector>

namespace act {

struct CliResult {
    int exit_code;
    std::string json;
};

class ComputerControlSystem final {
public:
    ComputerControlSystem();
    ~ComputerControlSystem();

    ComputerControlSystem(ComputerControlSystem&&) noexcept;
    ComputerControlSystem& operator=(ComputerControlSystem&&) noexcept;

    ComputerControlSystem(const ComputerControlSystem&) = delete;
    ComputerControlSystem& operator=(const ComputerControlSystem&) = delete;

    [[nodiscard]] CliResult execute(const std::vector<std::string>& arguments);

private:
    class Impl;
    std::unique_ptr<Impl> impl_;
};

}  // namespace act
