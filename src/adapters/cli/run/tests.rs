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
