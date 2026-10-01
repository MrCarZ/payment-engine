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
            .current_dir(&self.directory)
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

const SUMMARY_FIELDS: [&str; 6] = [
    "applied",
    "replayed",
    "rejected",
    "ignored",
    "input_errors",
    "processing_errors",
];

fn assert_summary(summary: &Value, expected: [u64; 6]) {
    for (field, count) in SUMMARY_FIELDS.into_iter().zip(expected) {
        assert_eq!(summary[field], count, "summary field {field}");
    }
}

fn assert_account_snapshot(bytes: &[u8], expected: &str) {
    assert_eq!(
        String::from_utf8_lossy(bytes).replace("\r\n", "\n"),
        expected.replace("\r\n", "\n")
    );
}

fn assert_trace_outcomes(records: &[csv::StringRecord], expected: [u64; 6]) {
    let mut counts = [0; 6];
    for record in &records[..records.len() - 1] {
        let attributes: Value = from_str(&record[6]).unwrap();
        let (index, outcome, replay) = match &record[3] {
            "payment.request_applied" => (0, Some("applied"), false),
            "payment.request_replayed" => (1, None, true),
            "payment.request_rejected" => (2, Some("rejected"), false),
            "payment.request_ignored" => (3, Some("ignored"), false),
            name => panic!("unexpected request event {name}"),
        };
        counts[index] += 1;
        assert_eq!(attributes["replayed"], replay);
        if let Some(outcome) = outcome {
            assert_eq!(attributes["outcome"], outcome);
        }
        if index == 2 || index == 3 {
            assert!(attributes["reason_code"].is_string());
        }
    }
    assert_eq!(counts, expected);
    let finished = records.last().unwrap();
    assert_eq!(&finished[3], "payment.run_finished");
    let attributes: Value = from_str(&finished[6]).unwrap();
    assert_eq!(attributes["status"], "completed");
    assert_summary(&attributes, expected);
}

#[rstest]
#[case::missing_input(&[], "Usage:")]
#[case::missing_batch_files(&["first.csv", "second.csv"], "cannot open input")]
fn cli_rejects_missing_arguments_or_files(#[case] args: &[&str], #[case] diagnostic: &str) {
    let fixture = Fixture::new("");
    let output = Command::new(env!("CARGO_BIN_EXE_payment-engine"))
        .current_dir(&fixture.directory)
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
    let fixture = Fixture::new(include_str!("fixtures/input/payments-a.csv"));
    let output = fixture.invoke();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = include_str!("fixtures/expected/single-accounts.csv");
    assert_account_snapshot(&output.stdout, expected);
    let accounts: Vec<_> = Reader::from_reader(output.stdout.as_slice())
        .records()
        .map(Result::unwrap)
        .collect();
    assert_eq!(&accounts[1][4], "true");
    assert_eq!(&accounts[2][1], "-8.0000");
    assert_eq!(&accounts[2][2], "10.0000");
    let traces = fixture.traces();
    assert_eq!(traces.len(), 1);
    let run = fixture.runs().pop().unwrap();
    assert_account_snapshot(&read(run.join("accounts.csv")).unwrap(), expected);
    assert!(!run.join("accounts.partial.csv").exists());
    let report: Value =
        from_str(&String::from_utf8(read(run.join("report.json")).unwrap()).unwrap()).unwrap();
    assert_eq!(report["status"], "completed");
    let run_id = report["run_id"].as_str().unwrap();
    assert_eq!(Uuid::parse_str(run_id).unwrap().get_version_num(), 4);
    assert_eq!(run.file_name().unwrap().to_str().unwrap(), run_id);
    assert_summary(&report["summary"], [10, 1, 2, 1, 0, 0]);
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
    assert_eq!(records.len(), 15);
    assert_trace_outcomes(&records, [10, 1, 2, 1, 0, 0]);
    let attributes: Value = from_str(&records.last().unwrap()[6]).unwrap();
    assert_eq!(attributes["status"], "completed");
    for record in &records {
        assert_eq!(&record[4], run_id);
        let attributes: Value = from_str(&record[6]).unwrap();
        assert_eq!(attributes["run_id"], run_id);
    }
    assert_eq!(attributes["applied"], 10);
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
    let run_ids: Vec<_> = fixture
        .runs()
        .iter()
        .map(|path| Uuid::parse_str(path.file_name().unwrap().to_str().unwrap()).unwrap())
        .collect();
    assert_ne!(run_ids[0], run_ids[1]);
    assert!(run_ids.iter().all(|id| id.get_version_num() == 4));
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
#[case("", false, 0)]
#[case("type,client,tx,amount\n", true, 0)]
#[case("type,client,tx,amount\ndeposit,1,1,0\n", false, 0)]
#[case("type,client,tx,amount\ndeposit,1,1,1.12345\n", false, 0)]
#[case(
    "type,client,tx,amount\ndeposit,1,1,1\ndeposit,1,2,bad\ndeposit,1,3,5\n",
    false,
    1
)]
fn cli_handles_empty_and_invalid_inputs(
    #[case] csv: &str,
    #[case] success: bool,
    #[case] applied: u64,
) {
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
        let run = fixture.runs().pop().unwrap();
        let report: Value =
            serde_json::from_slice(&read(run.join("report.json")).unwrap()).unwrap();
        assert_failed_account_artifacts(&run, &report);
        assert_summary(&report["summary"], [applied, 0, 0, 0, 1, 0]);
        let records: Vec<_> = Reader::from_reader(trace.as_bytes())
            .records()
            .map(Result::unwrap)
            .collect();
        assert_eq!(records.len() as u64, applied + 2);
        let failure: Value = from_str(&records[applied as usize][6]).unwrap();
        assert_eq!(&records[applied as usize][3], "payment.input_failed");
        assert_eq!(
            failure["record"],
            if csv.is_empty() { 0 } else { applied + 1 }
        );
        let finished: Value = from_str(&records.last().unwrap()[6]).unwrap();
        assert_eq!(finished["status"], "failed");
        assert_summary(&finished, [applied, 0, 0, 0, 1, 0]);
    }
}

fn assert_failed_account_artifacts(run: &Path, report: &Value) {
    assert_eq!(report["status"], "failed");
    assert_eq!(report["exit_code"], 1);
    assert!(report["account_file"].is_null());
    assert_eq!(report["partial_account_file"], "accounts.partial.csv");
    assert!(!run.join("accounts.csv").exists());
    assert!(run.join("accounts.partial.csv").is_file());
    assert!(read(run.join("accounts.partial.csv")).unwrap().is_empty());
    assert!(run.join("diagnostics.log").is_file());
}

#[test]
fn default_invocation_publishes_accounts_and_artifacts_under_the_working_directory() {
    let fixture = Fixture::new(include_str!("fixtures/input/payments.csv"));
    let output = Command::new(env!("CARGO_BIN_EXE_payment-engine"))
        .arg(&fixture.input)
        .current_dir(&fixture.directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_account_snapshot(
        &output.stdout,
        "client,available,held,total,locked\n3,4.0000,0.0000,4.0000,false\n9,4.0000,0.0000,4.0000,false\n",
    );
    let runs = fixture.runs();
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(read(run.join("accounts.csv")).unwrap(), output.stdout);
    assert!(!run.join("accounts.partial.csv").exists());
    let report: Value = serde_json::from_slice(&read(run.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["status"], "completed");
    assert_eq!(report["exit_code"], 0);
    assert_eq!(report["account_file"], "accounts.csv");
    assert_eq!(report["run_id"], run.file_name().unwrap().to_str().unwrap());
    assert_eq!(
        Uuid::parse_str(report["run_id"].as_str().unwrap())
            .unwrap()
            .get_version_num(),
        4
    );
    assert_summary(&report["summary"], [5, 0, 0, 0, 0, 0]);
    assert!(run.join("diagnostics.log").is_file());
    let traces = fixture.traces();
    assert_eq!(traces.len(), 1);
    let bytes = read(&traces[0]).unwrap();
    let records: Vec<_> = Reader::from_reader(bytes.as_slice())
        .records()
        .map(Result::unwrap)
        .collect();
    assert_trace_outcomes(&records, [5, 0, 0, 0, 0, 0]);
}

#[test]
fn missing_file_fails_without_stdout_or_trace() {
    let fixture = Fixture::new("");
    let output = Command::new(env!("CARGO_BIN_EXE_payment-engine"))
        .current_dir(&fixture.directory)
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
    let fixture = Fixture::new(include_str!("fixtures/input/payments-a.csv"));
    let other = fixture.directory.join("second.csv");
    write(&other, include_str!("fixtures/input/payments-b.csv")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_payment-engine"))
        .current_dir(&fixture.directory)
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
    assert_account_snapshot(
        &output.stdout,
        include_str!("fixtures/expected/batch-accounts.csv"),
    );
    let traces = fixture.traces();
    assert_eq!(traces.len(), 2);
    let mut run_ids = Vec::new();
    let mut source_ids = Vec::new();
    let mut applied = 0;
    for path in traces {
        let expected = if path.file_name().unwrap() == "source-0001.trace.csv" {
            [10, 1, 2, 1, 0, 0]
        } else {
            [4, 1, 1, 1, 0, 0]
        };
        let bytes = read(path).unwrap();
        let records: Vec<_> = Reader::from_reader(bytes.as_slice())
            .records()
            .map(Result::unwrap)
            .collect();
        assert_trace_outcomes(&records, expected);
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
    assert_eq!(applied, 14);
    assert_eq!(run_ids[0], run_ids[1]);
    assert_eq!(Uuid::parse_str(&run_ids[0]).unwrap().get_version_num(), 4);
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
    assert_eq!(read(run.join("accounts.csv")).unwrap(), output.stdout);
    assert!(!run.join("accounts.partial.csv").exists());
    assert!(run.join("diagnostics.log").is_file());
    assert_eq!(report["status"], "completed");
    assert_eq!(report["exit_code"], 0);
    assert_summary(&report["summary"], [14, 2, 3, 2, 0, 0]);
    assert_summary(&report["sources"][0]["summary"], [10, 1, 2, 1, 0, 0]);
    assert_summary(&report["sources"][1]["summary"], [4, 1, 1, 1, 0, 0]);
    for source in report["sources"].as_array().unwrap() {
        assert_eq!(source["status"], "completed");
        assert!(source["error"].is_null());
    }
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
        .current_dir(&fixture.directory)
        .arg("--output-dir")
        .arg(fixture.directory.join("output"))
        .args([&fixture.input, &other])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(fixture.traces().is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
    let run = fixture.runs().pop().unwrap();
    let report: Value = serde_json::from_slice(&read(run.join("report.json")).unwrap()).unwrap();
    assert_failed_account_artifacts(&run, &report);
    assert_summary(&report["summary"], [0; 6]);
    assert!(report["sources"].as_array().unwrap().is_empty());
    assert!(report["trace_files"].as_array().unwrap().is_empty());
    assert!(report["error"].as_str().unwrap().contains(diagnostic));
}

#[test]
fn duplicate_batch_input_is_rejected_before_trace_setup() {
    let fixture = Fixture::new("type,client,tx,amount\n");
    let output = Command::new(env!("CARGO_BIN_EXE_payment-engine"))
        .current_dir(&fixture.directory)
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
        .current_dir(&fixture.directory)
        .arg("--output-dir")
        .arg(fixture.directory.join("output"))
        .args([&fixture.input, &other])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(fixture.traces().len(), 2);
    assert!(String::from_utf8_lossy(&output.stderr).contains("batch source processing failed"));
    let run = fixture.runs().pop().unwrap();
    let report: Value = serde_json::from_slice(&read(run.join("report.json")).unwrap()).unwrap();
    assert_failed_account_artifacts(&run, &report);
    let sources = report["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0]["status"], "failed");
    assert!(sources[0]["error"].is_string());
    assert_summary(&sources[0]["summary"], [1, 0, 0, 0, 0, 1]);
    let peer_applied = sources[1]["summary"]["applied"].as_u64().unwrap();
    match peer_applied {
        0 => {
            assert_eq!(sources[1]["status"], "failed");
            assert!(
                sources[1]["error"]
                    .as_str()
                    .unwrap()
                    .contains("source skipped after batch failure")
            );
        }
        1 => {
            assert_eq!(sources[1]["status"], "completed");
            assert!(sources[1]["error"].is_null());
        }
        count => panic!("unexpected peer count {count}"),
    }
    assert_summary(&sources[1]["summary"], [peer_applied, 0, 0, 0, 0, 0]);
    assert_summary(&report["summary"], [1 + peer_applied, 0, 0, 0, 0, 1]);
    for field in SUMMARY_FIELDS {
        let sum: u64 = sources
            .iter()
            .map(|source| source["summary"][field].as_u64().unwrap())
            .sum();
        assert_eq!(report["summary"][field], sum);
    }
    for path in fixture.traces() {
        let bytes = read(path).unwrap();
        let records: Vec<_> = Reader::from_reader(bytes.as_slice())
            .records()
            .map(Result::unwrap)
            .collect();
        let last = records.last().unwrap();
        assert_eq!(&last[3], "payment.run_finished");
        let attributes: Value = from_str(&last[6]).unwrap();
        let source = sources
            .iter()
            .find(|source| source["source_id"] == attributes["source_id"])
            .unwrap();
        assert_eq!(attributes["status"], source["status"]);
        for field in SUMMARY_FIELDS {
            assert_eq!(attributes[field], source["summary"][field]);
        }
        assert_eq!(
            records.len() as u64,
            source["summary"]["applied"].as_u64().unwrap()
                + source["summary"]["processing_errors"].as_u64().unwrap()
                + 1
        );
    }
}
