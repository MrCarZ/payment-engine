use std::process::{Command, Output};

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_payment-engine"))
        .args(args)
        .output()
        .expect("binary should execute")
}

#[test]
fn missing_input_is_rejected_with_usage_on_stderr() {
    let output = invoke(&[]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
}

#[test]
fn extra_arguments_are_rejected() {
    let output = invoke(&["first.csv", "second.csv"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exactly one input path"));
}

#[test]
fn single_path_is_accepted_without_writing_to_stdout() {
    let output = invoke(&["path with spaces.csv"]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not implemented yet"));
}
