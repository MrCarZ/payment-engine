use super::{Report, Status};
use crate::manager::payment::run::Summary as ManagerSummary;
use rstest::rstest;
use serde_json::to_value;
use std::path::PathBuf;
#[rstest]
#[case(true)]
#[case(false)]
fn serializes_the_existing_report_schema_with_filename_only_references(#[case] success: bool) {
    let status = if success {
        Status::Completed
    } else {
        Status::Failed
    };
    let report = Report {
        run_id: "run-uuid".into(),
        exit_code: if success { 0 } else { 1 },
        status,
        elapsed_seconds: 0.5,
        input_files: Report::filenames(&[PathBuf::from("private/input/payments.csv")]),
        account_file: success.then(|| "accounts.csv".into()),
        partial_account_file: (!success).then(|| "accounts.partial.csv".into()),
        trace_files: Report::filenames(&[PathBuf::from("private/output/source-0001.trace.csv")]),
        summary: ManagerSummary {
            applied: 3,
            replayed: 1,
            ..ManagerSummary::default()
        }
        .into(),
        sources: Vec::new(),
        error: (!success).then(|| "failed".into()),
    };
    let value = to_value(&report).unwrap();
    assert_eq!(
        value["status"],
        if success { "completed" } else { "failed" }
    );
    assert_eq!(value["exit_code"], if success { 0 } else { 1 });
    assert_eq!(value["summary"]["applied"], 3);
    assert_eq!(value["summary"]["replayed"], 1);
    assert_eq!(value["input_files"][0], "payments.csv");
    assert_eq!(value["trace_files"][0], "source-0001.trace.csv");
    assert_eq!(value["account_file"].is_null(), !success);
    assert_eq!(value["partial_account_file"].is_null(), success);
    for field in ["input_paths", "accounts_path", "trace_paths"] {
        assert!(value.get(field).is_none());
    }
    assert!(!value.to_string().contains("private"));
}
