use rstest::rstest;

use crate::{
    adapters::{observability::memory::InMemoryTraceService, payment::csv::input::InputError},
    manager::payment::{batch::ValidationError, run::Coordinator},
};

use super::{BatchError, SourceContext, validate};

fn context(id: &str) -> SourceContext {
    SourceContext {
        run_id: "batch-1".into(),
        source_id: id.into(),
        partner_id: None,
    }
}

#[test]
fn snapshots_preserve_csv_positions_and_processing_order() {
    let first = "type,client,tx,amount\nresolve,1,1,\ndeposit,1,1,5\ndeposit,1,1,5\n";
    let second = "type,client,tx,amount\ndeposit,2,2,3\n";
    let batch = validate([
        (first.as_bytes(), context("a")),
        (second.as_bytes(), context("b")),
    ])
    .unwrap();
    assert_eq!(batch.sources()[0].records()[0].position.line, 2);
    assert_eq!(
        batch.sources()[1].records()[0].context.source.source_id,
        "b"
    );
    let mut trace = InMemoryTraceService::new();
    for source in batch.into_sources() {
        let (_, records) = source.into_parts();
        let mut coordinator = Coordinator::new();
        coordinator
            .process(records.into_iter().map(Ok::<_, InputError>), &mut trace)
            .unwrap();
    }
    assert_eq!(
        trace.events()[0].event.event_name,
        "payment.request_ignored"
    );
    assert_eq!(
        trace.events()[2].event.event_name,
        "payment.request_replayed"
    );
    assert_eq!(trace.events().len(), 4);
}

#[rstest]
#[case("", 0)]
#[case("type,client,tx,amount\ndeposit,2,2,bad\n", 1)]
fn malformed_later_source_fails_entire_preflight(#[case] csv: &str, #[case] record: u64) {
    let first = "type,client,tx,amount\ndeposit,1,1,5\n";
    let error = validate([
        (first.as_bytes(), context("a")),
        (csv.as_bytes(), context("b")),
    ])
    .unwrap_err();
    let BatchError::Input(error) = error else {
        panic!("expected input failure")
    };
    assert_eq!(error.context.source.source_id, "b");
    assert_eq!(error.position.record, record);
}

#[test]
fn parsed_sources_with_overlapping_clients_are_rejected() {
    let csv = "type,client,tx,amount\ndeposit,1,1,5\n";
    let other = "type,client,tx,amount\nresolve,1,999,\n";
    assert!(matches!(
        validate([
            (csv.as_bytes(), context("a")),
            (other.as_bytes(), context("b"))
        ]),
        Err(BatchError::Contract(ValidationError::ClientOverlap { .. }))
    ));
}
