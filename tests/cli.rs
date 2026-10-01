use std::{
    env::temp_dir,
    fs::{create_dir, read, read_dir, remove_dir, remove_file, write},
    path::{Path, PathBuf},
    process::{Command, Output, id},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use csv::Reader;
use rstest::rstest;
use serde_json::{Value, from_str};
use uuid::Uuid;

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
            .arg("--output-dir")
            .arg(self.directory.join("output"))
            .arg(&self.input)
            .output()
            .unwrap()
    }

    fn traces(&self) -> Vec<PathBuf> {
        self.runs()
            .into_iter()
            .flat_map(|directory| {
                read_dir(directory)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .collect::<Vec<_>>()
            })
            .filter(|path| path.to_string_lossy().ends_with(".trace.csv"))
            .collect()
    }

    fn runs(&self) -> Vec<PathBuf> {
        let output = self.directory.join("output");
        if !output.exists() {
            return Vec::new();
        }
        read_dir(output)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect()
    }
}

fn cleanup(path: &Path, root: &Path) {
    let resolved = path.canonicalize().unwrap();
    assert!(
        resolved.starts_with(root),
        "fixture cleanup must stay in its verified root"
    );
    if resolved.is_dir() {
        for entry in read_dir(&resolved).unwrap() {
            cleanup(&entry.unwrap().path(), root);
        }
        remove_dir(&resolved).unwrap();
    } else {
        remove_file(&resolved).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let root = self.directory.canonicalize().unwrap();
        cleanup(&root, &root);
    }
}

#[rstest]
#[case::missing_input(&[], "Usage:")]
#[case::missing_batch_files(&["first.csv", "second.csv"], "cannot open input")]
fn cli_rejects_missing_arguments_or_files(#[case] args: &[&str], #[case] diagnostic: &str) {
    let fixture = Fixture::new("");
    let output = Command::new(env!("CARGO_BIN_EXE_payment-engine"))
        .arg("--output-dir")
        .arg(fixture.directory.join("output"))
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
    let run = fixture.runs().pop().unwrap();
    assert_eq!(read(run.join("accounts.csv")).unwrap(), b"client,available,held,total,locked\n3,4.0000,0.0000,4.0000,false\n9,4.0000,0.0000,4.0000,false\n");
    assert!(!run.join("accounts.partial.csv").exists());
    let report: Value =
        from_str(&String::from_utf8(read(run.join("report.json")).unwrap()).unwrap()).unwrap();
    assert_eq!(report["status"], "completed");
    let run_id = report["run_id"].as_str().unwrap();
    assert_eq!(run.file_name().unwrap().to_str().unwrap(), run_id);
    assert_eq!(report["summary"]["applied"], 5);
    assert_eq!(report["account_file"], "accounts.csv");
    assert!(report["partial_account_file"].is_null());
    assert_eq!(report["trace_files"][0], "source-0001.trace.csv");
    assert_eq!(report["input_files"][0], "payments with spaces.csv");
    for field in [
        "accounts_path",
        "partial_accounts_path",
        "trace_paths",
        "input_paths",
    ] {
        assert!(report.get(field).is_none());
    }
    assert!(run.join("diagnostics.log").exists());
    assert!(!read_dir(&fixture.directory).unwrap().any(|entry| {
        entry
            .unwrap()
            .path()
            .to_string_lossy()
            .ends_with(".trace.csv")
    }));
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
    for record in &records {
        assert_eq!(&record[4], run_id);
        let attributes: Value = from_str(&record[6]).unwrap();
        assert_eq!(attributes["run_id"], run_id);
    }
    assert_eq!(attributes["applied"], 5);
    let canonical = fixture.input.canonicalize().unwrap();
    let expected_id = Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        canonical.as_os_str().as_encoded_bytes(),
    );
    assert_eq!(attributes["source_id"], expected_id.to_string());
    assert_eq!(expected_id.get_version_num(), 5);
    for record in &records {
        let attributes: Value = from_str(&record[6]).unwrap();
        assert_eq!(attributes["source_id"], expected_id.to_string());
    }
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
    assert_eq!(fixture.runs().len(), 2);
    assert_eq!(read(original_path).unwrap(), original);
    assert_eq!(read(&fixture.input).unwrap(), csv.as_bytes());
    let source_ids: Vec<_> = fixture
        .traces()
        .iter()
        .map(|path| {
            let bytes = read(path).unwrap();
            let record = Reader::from_reader(bytes.as_slice())
                .records()
                .next()
                .unwrap()
                .unwrap();
            let attributes: Value = from_str(&record[6]).unwrap();
            attributes["source_id"].as_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(source_ids[0], source_ids[1]);
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
        .arg("--output-dir")
        .arg(fixture.directory.join("output"))
        .arg(fixture.directory.join("missing.csv"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot open input"));
    assert!(fixture.traces().is_empty());
}

#[test]
fn cli_processes_disjoint_csvs_with_shared_run_identity_and_separate_traces() {
    let fixture = Fixture::new(include_str!("fixtures/input/payments.csv"));
    let other = fixture.directory.join("second.csv");
    write(&other, "type,client,tx,amount\ndeposit,2,77,2\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_payment-engine"))
        .arg("--output-dir")
        .arg(fixture.directory.join("output"))
        .args([&fixture.input, &other])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "client,available,held,total,locked\n2,2.0000,0.0000,2.0000,false\n3,4.0000,0.0000,4.0000,false\n9,4.0000,0.0000,4.0000,false\n"
    );
    let traces = fixture.traces();
    assert_eq!(traces.len(), 2);
    let mut run_ids = Vec::new();
    let mut source_ids = Vec::new();
    let mut applied = 0;
    for path in traces {
        let bytes = read(path).unwrap();
        let records: Vec<_> = Reader::from_reader(bytes.as_slice())
            .records()
            .map(Result::unwrap)
            .collect();
        let summary = records.last().unwrap();
        assert_eq!(&summary[3], "payment.run_finished");
        let attributes: Value = from_str(&summary[6]).unwrap();
        assert_eq!(attributes["status"], "completed");
        applied += attributes["applied"].as_u64().unwrap();
        run_ids.push(summary[4].to_owned());
        let source_id = attributes["source_id"].as_str().unwrap();
        assert_eq!(Uuid::parse_str(source_id).unwrap().get_version_num(), 5);
        source_ids.push(source_id.to_owned());
    }
    assert_eq!(applied, 6);
    assert_eq!(run_ids[0], run_ids[1]);
    assert_ne!(source_ids[0], source_ids[1]);
    let mut expected_ids: Vec<_> = [&fixture.input, &other]
        .iter()
        .map(|path| {
            let canonical = path.canonicalize().unwrap();
            Uuid::new_v5(
                &Uuid::NAMESPACE_URL,
                canonical.as_os_str().as_encoded_bytes(),
            )
            .to_string()
        })
        .collect();
    expected_ids.sort();
    source_ids.sort();
    assert_eq!(source_ids, expected_ids);
    let run = fixture.runs().pop().unwrap();
    let report: Value =
        from_str(&String::from_utf8(read(run.join("report.json")).unwrap()).unwrap()).unwrap();
    assert_eq!(
        report["trace_files"],
        from_str::<Value>(r#"["source-0001.trace.csv","source-0002.trace.csv"]"#).unwrap()
    );
    assert_eq!(
        report["sources"][0]["source_id"],
        Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            fixture
                .input
                .canonicalize()
                .unwrap()
                .as_os_str()
                .as_encoded_bytes()
        )
        .to_string()
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr)
            .matches("Trace log:")
            .count(),
        2
    );
}

#[rstest]
#[case("type,client,tx,amount\ndeposit,1,2,3\n", "client 1")]
#[case("type,client,tx,amount\ndeposit,2,1,3\n", "original transaction 1")]
#[case("type,client,tx,amount\nresolve,2,1,\n", "references transaction 1")]
#[case("type,client,tx,amount\ndeposit,2,2,bad\n", "batch input failed")]
fn batch_preflight_failures_create_no_traces_or_account_output(
    #[case] csv: &str,
    #[case] diagnostic: &str,
) {
    let fixture = Fixture::new("type,client,tx,amount\ndeposit,1,1,5\n");
    let other = fixture.directory.join("second.csv");
    write(&other, csv).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_payment-engine"))
        .arg("--output-dir")
        .arg(fixture.directory.join("output"))
        .args([&fixture.input, &other])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(fixture.traces().is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
}

#[test]
fn duplicate_batch_input_is_rejected_before_trace_setup() {
    let fixture = Fixture::new("type,client,tx,amount\n");
    let output = Command::new(env!("CARGO_BIN_EXE_payment-engine"))
        .arg("--output-dir")
        .arg(fixture.directory.join("output"))
        .args([
            &fixture.input,
            &fixture.directory.join(".").join("payments with spaces.csv"),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(fixture.traces().is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("duplicate batch source"));
    let run = fixture.runs().pop().unwrap();
    assert!(!run.join("accounts.csv").exists());
    assert!(run.join("accounts.partial.csv").exists());
    let report: Value =
        from_str(&String::from_utf8(read(run.join("report.json")).unwrap()).unwrap()).unwrap();
    assert_eq!(report["status"], "failed");
    assert_eq!(report["summary"]["applied"], 0);
    assert!(report["account_file"].is_null());
    assert_eq!(report["partial_account_file"], "accounts.partial.csv");
    assert!(
        !report["error"]
            .as_str()
            .unwrap()
            .contains(&fixture.directory.to_string_lossy().into_owned())
    );
    assert!(
        report["error"]
            .as_str()
            .unwrap()
            .contains("duplicate batch source")
    );
}

#[test]
fn failed_batch_worker_suppresses_aggregate_accounts_and_keeps_traces() {
    let fixture = Fixture::new(
        "type,client,tx,amount\ndeposit,1,1,17014118346046923173168730371588410.5727\ndeposit,1,2,1\n",
    );
    let other = fixture.directory.join("second.csv");
    write(&other, "type,client,tx,amount\ndeposit,2,3,1\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_payment-engine"))
        .arg("--output-dir")
        .arg(fixture.directory.join("output"))
        .args([&fixture.input, &other])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(fixture.traces().len(), 2);
    assert!(String::from_utf8_lossy(&output.stderr).contains("batch source processing failed"));
    for path in fixture.traces() {
        let csv = String::from_utf8(read(path).unwrap()).unwrap();
        assert!(csv.contains("payment.run_finished"));
    }
}
