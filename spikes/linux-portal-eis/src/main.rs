//! 运行 Linux Portal EIS 零输入、零像素连通验证。

mod contract;
mod portal;

use std::process::ExitCode;

use contract::{CliAction, FailureSummary, HELP, SuccessSummary, parse_cli};

fn print_json<T: serde::Serialize>(value: &T) -> bool {
    match serde_json::to_string(value) {
        Ok(text) => {
            println!("{text}");
            true
        }
        Err(_) => {
            println!(
                "{{\"contractVersion\":\"linux-portal-eis-spike/v1\",\"outcome\":\"failed\",\"code\":\"OUTPUT_ENCODING_FAILED\",\"stage\":\"output\",\"sessionClosed\":false,\"restoreTokenRetained\":false,\"inputEventsSent\":0,\"pixelsConsumed\":0}}"
            );
            false
        }
    }
}

fn main() -> ExitCode {
    let action = match parse_cli(std::env::args().skip(1)) {
        Ok(action) => action,
        Err(failure) => {
            let _ = print_json(&FailureSummary::new(failure, false));
            return ExitCode::from(2);
        }
    };
    let CliAction::Run(config) = action else {
        println!("{HELP}");
        return ExitCode::SUCCESS;
    };

    match async_io::block_on(portal::verify(config)) {
        Ok(verification) => {
            if print_json(&SuccessSummary::from(verification)) {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            }
        }
        Err(failure) => {
            let _ = print_json(&FailureSummary::new(
                failure.failure,
                failure.session_closed,
            ));
            ExitCode::from(2)
        }
    }
}
