use std::{env, process::ExitCode};

use payment_engine::runtime::InputConfig;

fn main() -> ExitCode {
    match InputConfig::from_args(env::args_os().skip(1)) {
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
