use std::{env::args_os, io::stdout, process::ExitCode};

use payment_engine::bootstrap::{artifacts::execute, config::Invocation};

fn main() -> ExitCode {
    match Invocation::from_args(args_os().skip(1)) {
        Ok(invocation) => match execute(invocation, stdout().lock()) {
            Ok(execution) => {
                eprintln!("Output directory: {}", execution.directory.display());
                for path in execution.trace_paths {
                    eprintln!("Trace log: {}", path.display());
                }
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
