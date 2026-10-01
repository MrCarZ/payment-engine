use payment_engine::bootstrap::{
    config::Config,
    payment::{batch::execute as execute_batch, execute},
};
use std::{env::args_os, io::stdout, process::ExitCode};
fn main() -> ExitCode {
    let result = match Config::from_args(args_os().skip(1)) {
        Ok(Config::Single(config)) => execute(config, stdout().lock())
            .map(|execution| vec![execution.trace_path])
            .map_err(|error| error.to_string()),
        Ok(Config::Batch(config)) => execute_batch(config, stdout().lock())
            .map(|execution| execution.trace_paths)
            .map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    match result {
        Ok(paths) => {
            for path in paths {
                eprintln!("Trace log: {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
