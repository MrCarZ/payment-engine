use std::sync::Arc;

use rstest::rstest;

use crate::{
    domain::payment::{ClientId, LifecycleAction, TransactionId, transaction::Type},
    manager::payment::{Context, Request, SourceContext},
};

use super::{Record, Source, ValidatedBatch, ValidationError};

#[derive(Debug)]
struct Envelope {
    request: Request,
    context: Context,
}

impl Record for Envelope {
    fn request(&self) -> Request {
        self.request
    }
    fn context(&self) -> &Context {
        &self.context
    }
}

fn original(client: u16, tx: u32) -> Request {
    Request::Original {
        client: ClientId::from(client),
        tx: TransactionId::from(tx),
        transaction_type: Type::Deposit,
        amount: "1".parse().unwrap(),
    }
}

fn lifecycle(client: u16, tx: u32) -> Request {
    Request::Lifecycle {
        client: ClientId::from(client),
        tx: TransactionId::from(tx),
        action: LifecycleAction::Resolve,
    }
}

fn source(id: &str, requests: Vec<Request>) -> Source<Envelope> {
    let context = SourceContext {
        run_id: "batch-1".into(),
        source_id: id.into(),
        partner_id: None,
    };
    let shared = Arc::new(context.clone());
    let records = requests
        .into_iter()
        .map(|request| Envelope {
            request,
            context: Context {
                source: Arc::clone(&shared),
            },
        })
        .collect();
    Source::new(context, records)
}

#[test]
fn disjoint_sources_preserve_order_and_allow_same_source_replays_and_conflicts() {
    let requests = vec![
        lifecycle(1, 1),
        original(1, 1),
        original(1, 1),
        original(3, 1),
    ];
    let batch = ValidatedBatch::try_from(vec![
        source("a", requests.clone()),
        source("b", vec![original(2, 2)]),
    ])
    .unwrap();
    assert_eq!(batch.sources()[0].context().source_id, "a");
    assert_eq!(
        batch.sources()[0]
            .records()
            .iter()
            .map(Record::request)
            .collect::<Vec<_>>(),
        requests
    );
    let sources = batch.into_sources();
    let (context, records) = sources.into_iter().next().unwrap().into_parts();
    assert_eq!(context.source_id, "a");
    assert_eq!(records.len(), 4);
}

#[test]
fn empty_batch_is_rejected_but_empty_sources_are_valid() {
    assert_eq!(
        ValidatedBatch::<Envelope>::try_from(Vec::new()).unwrap_err(),
        ValidationError::EmptyBatch
    );
    let batch = ValidatedBatch::try_from(vec![source("a", vec![]), source("b", vec![])]).unwrap();
    assert_eq!(batch.sources().len(), 2);
}

#[rstest]
#[case(original(1, 2))]
#[case(lifecycle(1, 99))]
fn every_request_reserves_its_client_to_the_source(#[case] request: Request) {
    let error = ValidatedBatch::try_from(vec![
        source("a", vec![original(1, 1)]),
        source("b", vec![request]),
    ])
    .unwrap_err();
    let ValidationError::ClientOverlap {
        client,
        first,
        conflicting,
    } = error
    else {
        panic!("expected client overlap")
    };
    assert_eq!(client, ClientId::from(1));
    assert_eq!(first.source_id, "a");
    assert_eq!(conflicting.source_id, "b");
    assert_eq!(first.record, 1);
    assert_eq!(conflicting.record, 1);
}

#[rstest]
#[case(Type::Deposit)]
#[case(Type::Withdrawal)]
fn originals_cannot_share_an_id_across_sources_even_with_disjoint_clients(
    #[case] transaction_type: Type,
) {
    let duplicate = Request::Original {
        client: ClientId::from(2),
        tx: TransactionId::from(7),
        transaction_type,
        amount: "1".parse().unwrap(),
    };
    let error = ValidatedBatch::try_from(vec![
        source("a", vec![original(1, 7)]),
        source("b", vec![original(2, 8), duplicate]),
    ])
    .unwrap_err();
    let ValidationError::TransactionOverlap {
        tx,
        first,
        conflicting,
    } = error
    else {
        panic!("expected original overlap")
    };
    assert_eq!(tx, TransactionId::from(7));
    assert_eq!(first.source_id, "a");
    assert_eq!(conflicting.record, 2);
}

#[rstest]
#[case(false)]
#[case(true)]
fn cross_source_references_are_detected_regardless_of_source_order(#[case] reverse: bool) {
    let mut sources = vec![
        source("a", vec![lifecycle(1, 7)]),
        source("b", vec![original(2, 7)]),
    ];
    if reverse {
        sources.reverse();
    }
    let error = ValidatedBatch::try_from(sources).unwrap_err();
    let ValidationError::CrossSourceReference {
        tx,
        original,
        reference,
    } = error
    else {
        panic!("expected reference overlap")
    };
    assert_eq!(tx, TransactionId::from(7));
    assert_eq!(original.source_id, "b");
    assert_eq!(reference.source_id, "a");
}

#[test]
fn globally_unknown_references_remain_independent_business_outcomes() {
    assert!(
        ValidatedBatch::try_from(vec![
            source("a", vec![lifecycle(1, 99)]),
            source("b", vec![lifecycle(2, 99)])
        ])
        .is_ok()
    );
}

#[test]
fn duplicate_source_ids_are_rejected_even_without_rows() {
    assert_eq!(
        ValidatedBatch::try_from(vec![source("a", vec![]), source("a", vec![])]).unwrap_err(),
        ValidationError::DuplicateSource {
            source_id: "a".into()
        }
    );
}

#[test]
fn all_sources_must_belong_to_one_run() {
    let (mut context, records) = source("b", vec![]).into_parts();
    context.run_id = "other-run".into();
    let other = Source::new(context, records);
    let error = ValidatedBatch::try_from(vec![source("a", vec![]), other]).unwrap_err();
    assert!(matches!(error, ValidationError::RunMismatch { .. }));
}

#[test]
fn forged_record_context_cannot_override_source_ownership() {
    let (context, mut records) = source("a", vec![original(1, 1)]).into_parts();
    records[0].context.source = Arc::new(SourceContext {
        run_id: "batch-1".into(),
        source_id: "b".into(),
        partner_id: None,
    });
    let error = ValidatedBatch::try_from(vec![Source::new(context, records)]).unwrap_err();
    assert!(matches!(error, ValidationError::ContextMismatch { .. }));
}
