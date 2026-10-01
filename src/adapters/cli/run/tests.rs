use super::{ArtifactError, discover_traces, run};
use crate::adapters::cli::config::{Config, InputConfig, Invocation};
use rstest::rstest;
use serde_json::{Value, from_slice};
use std::{
    env::temp_dir,
    fs::{create_dir, read, remove_dir_all, write},
    io::{Error as IoError, Result as IoResult, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = temp_dir().join(format!("payment-artifacts-{}", Uuid::new_v4()));
        create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let path = self.0.canonicalize().unwrap();
        assert!(path.starts_with(temp_dir().canonicalize().unwrap()));
        remove_dir_all(path).unwrap();
    }
}

struct CollisionWriter {
    path: PathBuf,
    created: bool,
}
impl Write for CollisionWriter {
    fn write(&mut self, bytes: &[u8]) -> IoResult<usize> {
        Ok(bytes.len())
    }
    fn flush(&mut self) -> IoResult<()> {
        if !self.created {
            create_dir(&self.path)?;
            write(self.path.join("sentinel"), b"preserve collision")?;
            self.created = true;
        }
        Ok(())
    }
}

#[rstest]
fn diagnostics_failure_is_reported_without_losing_processing_results(
    #[values(true, false)] valid: bool,
) {
    let fixture = Directory::new();
    let input = fixture.0.join("input.csv");
    let amount = if valid { "1" } else { "invalid" };
    write(
        &input,
        format!("type,client,tx,amount\ndeposit,1,1,{amount}\n"),
    )
    .unwrap();
    let output_root = fixture.0.join("output");
    let directory = output_root.join("run");
    let writer = CollisionWriter {
        path: directory.join("diagnostics.log"),
        created: false,
    };
    let error = run(
        Invocation {
            config: Config::Single(InputConfig { input_path: input }),
            output_root,
        },
        writer,
        "run".into(),
    )
    .unwrap_err();
    let report: Value = from_slice(&read(directory.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["status"], "failed");
    assert_eq!(report["exit_code"], 1);
    assert_eq!(report["summary"]["applied"], u64::from(valid));
    assert_eq!(report["summary"]["input_errors"], u64::from(!valid));
    assert_eq!(report["account_file"].is_null(), !valid);
    assert_eq!(report["partial_account_file"].is_null(), valid);
    assert!(matches!(error, ArtifactError::Run { .. }));
    let message = report["error"].as_str().unwrap();
    if !valid {
        assert!(message.contains("amount"));
        assert!(message.contains("additionally"));
    }
    assert!(message.contains("artifact I/O"));
}

#[test]
fn report_publication_failure_keeps_only_a_partial_report() {
    let fixture = Directory::new();
    let input = fixture.0.join("input.csv");
    write(&input, "type,client,tx,amount\ndeposit,1,1,1\n").unwrap();
    let output_root = fixture.0.join("output");
    let directory = output_root.join("run");
    let writer = CollisionWriter {
        path: directory.join("report.json"),
        created: false,
    };
    let error = run(
        Invocation {
            config: Config::Single(InputConfig { input_path: input }),
            output_root,
        },
        writer,
        "run".into(),
    )
    .unwrap_err();
    assert!(matches!(error, ArtifactError::Run { .. }));
    assert!(directory.join("report.json").is_dir());
    assert!(directory.join("report.partial.json").is_file());
    assert!(directory.join("accounts.csv").is_file());
}

#[test]
fn trace_discovery_failure_preserves_the_primary_error() {
    let mut result = Err(ArtifactError::from(IoError::other(
        "primary processing failure",
    )));
    let traces = discover_traces(Path::new("unused"), &mut result, |_| {
        Err(IoError::other("trace discovery failed"))
    });
    assert!(traces.is_empty());
    let error = result.unwrap_err();
    assert!(matches!(error, ArtifactError::Additional { .. }));
    assert!(error.to_string().contains("primary processing failure"));
    assert!(error.to_string().contains("trace discovery failed"));
}

fn fixture_invocation(fixture: &Directory) -> Invocation {
    let input = fixture.0.join("input.csv");
    write(&input, "type,client,tx,amount\ndeposit,1,1,1\n").unwrap();
    Invocation {
        config: Config::Single(InputConfig { input_path: input }),
        output_root: fixture.0.join("output"),
    }
}

#[test]
fn output_root_file_is_preserved_and_setup_writes_no_account_output() {
    let fixture = Directory::new();
    let invocation = fixture_invocation(&fixture);
    write(&invocation.output_root, b"preserve output root").unwrap();
    let mut output = Vec::new();
    let error = run(invocation, &mut output, "run".into()).unwrap_err();
    assert!(error.to_string().contains("create run directory"));
    assert!(output.is_empty());
    assert_eq!(
        read(fixture.0.join("output")).unwrap(),
        b"preserve output root"
    );
    assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 2);
}

#[test]
fn existing_run_directory_preserves_artifacts_and_writes_no_account_output() {
    let fixture = Directory::new();
    let invocation = fixture_invocation(&fixture);
    create_dir(&invocation.output_root).unwrap();
    let directory = invocation.output_root.join("run");
    create_dir(&directory).unwrap();
    let sentinel = directory.join("accounts.partial.csv");
    write(&sentinel, b"existing artifact").unwrap();
    let mut output = Vec::new();
    let error = run(invocation, &mut output, "run".into()).unwrap_err();
    assert!(error.to_string().contains("create run directory"));
    assert!(output.is_empty());
    assert_eq!(read(sentinel).unwrap(), b"existing artifact");
    assert_eq!(std::fs::read_dir(directory).unwrap().count(), 1);
}

struct ShortWriter {
    bytes: Vec<u8>,
    writes: usize,
    fail_after: Option<usize>,
}
impl Write for ShortWriter {
    fn write(&mut self, bytes: &[u8]) -> IoResult<usize> {
        let remaining = self
            .fail_after
            .map_or(usize::MAX, |limit| limit.saturating_sub(self.bytes.len()));
        if remaining == 0 {
            return Err(IoError::other("stdout failed after prefix"));
        }
        let count = bytes.len().min(3).min(remaining);
        self.bytes.extend_from_slice(&bytes[..count]);
        self.writes += 1;
        Ok(count)
    }
    fn flush(&mut self) -> IoResult<()> {
        Ok(())
    }
}

const SINGLE_ACCOUNT: &[u8] = b"client,available,held,total,locked\n1,1.0000,0.0000,1.0000,false\n";

#[test]
fn tee_short_writes_deliver_complete_stdout_and_matching_saved_accounts() {
    let fixture = Directory::new();
    let mut output = ShortWriter {
        bytes: Vec::new(),
        writes: 0,
        fail_after: None,
    };
    let execution = run(fixture_invocation(&fixture), &mut output, "run".into()).unwrap();
    assert_eq!(output.bytes, SINGLE_ACCOUNT);
    assert!(output.writes > 1);
    assert_eq!(
        read(execution.directory.join("accounts.csv")).unwrap(),
        output.bytes
    );
    assert!(!execution.directory.join("accounts.partial.csv").exists());
    let report: Value =
        from_slice(&read(execution.directory.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["status"], "completed");
    assert_eq!(report["exit_code"], 0);
    assert_eq!(report["account_file"], "accounts.csv");
    assert!(report["partial_account_file"].is_null());
    assert_eq!(report["summary"]["applied"], 1);
}

#[test]
fn tee_stdout_failure_retains_partial_accounts_and_reports_failed_publication() {
    let fixture = Directory::new();
    let mut output = ShortWriter {
        bytes: Vec::new(),
        writes: 0,
        fail_after: Some(7),
    };
    let error = run(fixture_invocation(&fixture), &mut output, "run".into()).unwrap_err();
    assert!(error.to_string().contains("stdout failed after prefix"));
    assert_eq!(output.bytes, &SINGLE_ACCOUNT[..7]);
    assert!(output.writes > 1);
    let directory = fixture.0.join("output/run");
    assert_eq!(
        read(directory.join("accounts.partial.csv")).unwrap(),
        SINGLE_ACCOUNT
    );
    assert!(!directory.join("accounts.csv").exists());
    let report: Value = from_slice(&read(directory.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["status"], "failed");
    assert_eq!(report["exit_code"], 1);
    assert!(report["account_file"].is_null());
    assert_eq!(report["partial_account_file"], "accounts.partial.csv");
    assert_eq!(report["summary"]["applied"], 1);
    assert!(
        report["error"]
            .as_str()
            .unwrap()
            .contains("stdout failed after prefix")
    );
    assert!(directory.join("diagnostics.log").is_file());
}

#[test]
fn accounts_publication_collision_preserves_directory_and_reports_partial_filename() {
    let fixture = Directory::new();
    let directory = fixture.0.join("output/run");
    let writer = CollisionWriter {
        path: directory.join("accounts.csv"),
        created: false,
    };
    let error = run(fixture_invocation(&fixture), writer, "run".into()).unwrap_err();
    assert!(matches!(error, ArtifactError::Run { .. }));
    assert!(error.to_string().contains("artifact I/O"));
    assert!(directory.join("accounts.csv").is_dir());
    assert_eq!(
        read(directory.join("accounts.csv/sentinel")).unwrap(),
        b"preserve collision"
    );
    assert_eq!(
        read(directory.join("accounts.partial.csv")).unwrap(),
        SINGLE_ACCOUNT
    );
    let report: Value = from_slice(&read(directory.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["status"], "failed");
    assert_eq!(report["exit_code"], 1);
    assert!(report["account_file"].is_null());
    assert_eq!(report["partial_account_file"], "accounts.partial.csv");
    assert_eq!(report["summary"]["applied"], 1);
    assert!(report["error"].as_str().unwrap().contains("artifact I/O"));
}
