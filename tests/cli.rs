use std::{
    env::temp_dir,
    fs::{create_dir, read, read_dir, remove_dir, remove_file, write},
    path::PathBuf,
    process::{Command, Output, id},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use csv::Reader;
use rstest::rstest;
use serde_json::{Value, from_str};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    directory: PathBuf,
    input: PathBuf,
}

impl Fixture {
    fn new(csv: &str) -> Self {
        let unique = format!(
            "payment-engine-cli-{}-{}-{}",
            id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let directory = temp_dir().join(unique);
        create_dir(&directory).unwrap();
        let input = directory.join("payments with spaces.csv");
        write(&input, csv).unwrap();
        Self { directory, input }
    }

    fn invoke(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_payment-engine"))
            .arg(&self.input)
            .output()
            .unwrap()
    }

    fn traces(&self) -> Vec<PathBuf> {
        read_dir(&self.directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.to_string_lossy().ends_with(".trace.csv"))
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only this fixture's immediate files; no recursive cleanup.
        for entry in read_dir(&self.directory).unwrap() {
            let path = entry.unwrap().path();
            assert_eq!(path.parent(), Some(self.directory.as_path()));
            remove_file(path).unwrap();
        }
        remove_dir(&self.directory).unwrap();
    }
}

#[rstest]
#[case::missing_input(&[], "Usage:")]
#[case::extra_arguments(&["first.csv", "second.csv"], "exactly one input path")]
fn cli_rejects_invalid_arguments(#[case] args: &[&str], #[case] diagnostic: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_payment-engine"))
        .args(args)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
}

#[test]
fn successful_cli_outputs_accounts_and_creates_a_separate_csv_trace() {
    let fixture = Fixture::new(include_str!("fixtures/input/payments.csv"));
    let output = fixture.invoke();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "client,available,held,total,locked\n3,4.0000,0.0000,4.0000,false\n9,4.0000,0.0000,4.0000,false\n"
    );
    let traces = fixture.traces();
    assert_eq!(traces.len(), 1);
    let bytes = read(&traces[0]).unwrap();
    let mut reader = Reader::from_reader(bytes.as_slice());
    assert_eq!(
        reader.headers().unwrap().iter().collect::<Vec<_>>(),
        [
            "timestamp",
            "severity",
            "component",
            "event_name",
            "correlation_id",
            "message",
            "attributes"
        ]
    );
    let records: Vec<_> = reader.records().map(Result::unwrap).collect();
    assert_eq!(records.len(), 6);
    assert_eq!(&records[5][3], "payment.run_finished");
    let attributes: Value = from_str(&records[5][6]).unwrap();
    assert_eq!(attributes["status"], "completed");
    assert_eq!(attributes["applied"], 5);
    assert!(String::from_utf8_lossy(&output.stderr).contains("Trace log:"));
}

#[test]
fn repeated_invocations_preserve_existing_traces_and_input() {
    let csv = "type,client,tx,amount\n";
    let fixture = Fixture::new(csv);
    assert!(fixture.invoke().status.success());
    let original_path = fixture.traces().pop().unwrap();
    let original = read(&original_path).unwrap();
    assert!(fixture.invoke().status.success());
    assert_eq!(fixture.traces().len(), 2);
    assert_eq!(read(original_path).unwrap(), original);
    assert_eq!(read(&fixture.input).unwrap(), csv.as_bytes());
}

#[rstest]
#[case("", false)]
#[case("type,client,tx,amount\n", true)]
#[case("type,client,tx,amount\ndeposit,1,1,0\n", false)]
#[case("type,client,tx,amount\ndeposit,1,1,1.12345\n", false)]
#[case("type,client,tx,amount\ndeposit,1,1,1\ndeposit,1,2,bad\n", false)]
fn cli_handles_empty_and_invalid_inputs(#[case] csv: &str, #[case] success: bool) {
    let fixture = Fixture::new(csv);
    let output = fixture.invoke();
    assert_eq!(output.status.success(), success);
    if success {
        assert_eq!(output.stdout, b"client,available,held,total,locked\n");
    } else {
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("input failed"));
        let trace = String::from_utf8(read(&fixture.traces()[0]).unwrap()).unwrap();
        assert!(trace.contains("payment.input_failed"));
        assert!(trace.contains("payment.run_finished"));
    }
}

#[test]
fn missing_file_fails_without_stdout_or_trace() {
    let fixture = Fixture::new("");
    let output = Command::new(env!("CARGO_BIN_EXE_payment-engine"))
        .arg(fixture.directory.join("missing.csv"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot open input"));
    assert!(fixture.traces().is_empty());
}
