use std::process::{Command, Output};

use rstest::rstest;

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_payment-engine"))
        .args(args)
        .output()
        .expect("binary should execute")
}

#[rstest]
#[case::missing_input(&[], false, "Usage:")]
#[case::extra_arguments(&["first.csv", "second.csv"], false, "exactly one input path")]
#[case::single_path(&["path with spaces.csv"], true, "not implemented yet")]
fn cli_validates_arguments_and_reserves_stdout(
    #[case] args: &[&str],
    #[case] success: bool,
    #[case] diagnostic: &str,
) {
    let output = invoke(args);
    assert_eq!(output.status.success(), success);
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
}
