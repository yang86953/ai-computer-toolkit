//! 非默认 Linux AT-SPI 候选 observation worker 二进制入口。

fn main() {
    std::process::exit(ai_computer_toolkit::atspi_observation_worker::run_stdio());
}
