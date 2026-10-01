use std::{env::args_os, io::stdout, process::ExitCode};

use payment_engine::bootstrap::{config::InputConfig, payment::execute};

fn main() -> ExitCode {
    match InputConfig::from_args(args_os().skip(1)) {
        Ok(config) => match execute(config, stdout().lock()) {
            Ok(execution) => {
                eprintln!("Trace log: {}", execution.trace_path.display());
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
