use std::{env::args_os, process::ExitCode};

use payment_engine::runtime::InputConfig;

fn main() -> ExitCode {
    match InputConfig::from_args(args_os().skip(1)) {
        Ok(_config) => {
            eprintln!("Input argument accepted; CSV processing is not implemented yet.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
