use crate::manager::{
    observability::TraceService,
    payment::{
        SourceContext,
        run::{Coordinator, Record, RunFailure as SourceFailure},
    },
};
use std::{
    collections::VecDeque,
    convert::Infallible,
    io::Error as IoError,
    num::NonZeroUsize,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex},
    thread::{Builder, scope},
};

pub(super) struct Job<R, T> {
    pub(super) index: usize,
    pub(super) source: SourceContext,
    pub(super) records: Vec<R>,
    pub(super) trace: T,
}
type SharedJob<R, T> = Arc<Mutex<Option<Job<R, T>>>>;
pub(super) struct Completed<R, T, O> {
    pub(super) index: usize,
    pub(super) source: SourceContext,
    pub(super) coordinator: Coordinator,
    pub(super) trace: Option<T>,
    pub(super) result: Result<(), SourceFailure<R, Infallible, O>>,
}

pub(super) fn execute<R: Record + Send, T: TraceService + Send, O: Send>(
    mut pending: VecDeque<Job<R, T>>,
    workers: NonZeroUsize,
) -> Vec<Completed<R, T, O>> {
    let mut completed = Vec::new();
    while !pending.is_empty() {
        let mut group = execute_group(&mut pending, workers);
        let failed = group.iter().any(|source| source.result.is_err());
        completed.append(&mut group);
        if failed {
            break;
        }
    }
    cancel_pending(pending, &mut completed);
    completed.sort_by_key(|source| source.index);
    completed
}

fn execute_group<R: Record + Send, T: TraceService + Send, O: Send>(
    pending: &mut VecDeque<Job<R, T>>,
    workers: NonZeroUsize,
) -> Vec<Completed<R, T, O>> {
    scope(|scope| {
        let mut handles = Vec::new();
        let mut results = Vec::new();
        for _ in 0..workers.get() {
            let Some(job) = pending.pop_front() else {
                break;
            };
            let index = job.index;
            let source = job.source.clone();
            match launch(job, |shared| {
                Builder::new()
                    .name(format!("payment-source-{index}"))
                    .spawn_scoped(scope, move || {
                        let job = shared.lock().unwrap().take().unwrap();
                        process(job)
                    })
            }) {
                Ok(handle) => handles.push((index, source, handle)),
                Err(failure) => {
                    results.push(*failure);
                    break;
                }
            }
        }
        for (index, source, handle) in handles {
            results.push(match handle.join() {
                Ok(result) => result,
                Err(_) => Completed::failed(index, source, SourceFailure::WorkerPanicked),
            });
        }
        results
    })
}

// Retain ownership until the thread has been successfully created. A failed
// spawn drops its closure, but must leave the trace available for finalization.
pub(super) fn launch<R, T, O, H>(
    job: Job<R, T>,
    spawn: impl FnOnce(SharedJob<R, T>) -> Result<H, IoError>,
) -> Result<H, Box<Completed<R, T, O>>> {
    let shared = Arc::new(Mutex::new(Some(job)));
    match spawn(Arc::clone(&shared)) {
        Ok(handle) => Ok(handle),
        Err(error) => {
            let job = shared.lock().unwrap().take().unwrap();
            Err(Box::new(Completed {
                index: job.index,
                source: job.source,
                coordinator: Coordinator::new(),
                trace: Some(job.trace),
                result: Err(SourceFailure::WorkerSpawn(error)),
            }))
        }
    }
}

fn cancel_pending<R, T, O>(pending: VecDeque<Job<R, T>>, completed: &mut Vec<Completed<R, T, O>>) {
    for job in pending {
        completed.push(Completed {
            index: job.index,
            source: job.source,
            coordinator: Coordinator::new(),
            trace: Some(job.trace),
            result: Err(SourceFailure::Cancelled),
        });
    }
}

fn process<R: Record, T: TraceService, O>(mut job: Job<R, T>) -> Completed<R, T, O> {
    let mut coordinator = Coordinator::new();
    let result = match catch_unwind(AssertUnwindSafe(|| {
        coordinator
            .process(
                job.records.into_iter().map(Ok::<_, Infallible>),
                &mut job.trace,
            )
            .map_err(SourceFailure::Coordinator)
            .and_then(|()| job.trace.flush().map_err(SourceFailure::Trace))
    })) {
        Ok(result) => result,
        Err(_) => Err(SourceFailure::WorkerPanicked),
    };
    Completed {
        index: job.index,
        source: job.source,
        coordinator,
        trace: Some(job.trace),
        result,
    }
}

impl<R, T, O> Completed<R, T, O> {
    fn failed(
        index: usize,
        source: SourceContext,
        failure: SourceFailure<R, Infallible, O>,
    ) -> Self {
        Self {
            index,
            source,
            coordinator: Coordinator::new(),
            trace: None,
            result: Err(failure),
        }
    }
}
