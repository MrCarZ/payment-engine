//! Bounded execution of validated transport-independent source snapshots.

use super::ValidatedBatch;
use crate::manager::{
    observability::{TraceError, TraceService},
    payment::{
        SourceContext,
        run::{Output, Record},
    },
};
use std::num::NonZeroUsize;

mod completion;
mod error;
mod preparation;
mod report;
mod workers;

use completion::{complete, finalize, publish};
pub use error::{ExecutionError, Failure};
use preparation::prepare;
pub use report::{Report, SourceReport};
use workers::execute;

/// Prepares every trace before processing bounded worker groups. Publication
/// follows successful processing; all available traces are finalized afterward.
pub fn run<R: Record + Send, T: TraceService + Send, O: Output>(
    batch: ValidatedBatch<R>,
    output: &mut O,
    workers: NonZeroUsize,
    trace_factory: impl FnMut(&SourceContext) -> Result<T, TraceError>,
) -> Result<Report<R, O::Error>, ExecutionError<R, O::Error>>
where
    O::Error: Send,
{
    let pending = prepare(batch, trace_factory)?;
    let completed = execute(pending, workers);
    let output_result = publish(&completed, output);
    let report = finalize(completed, output_result.is_err());
    complete(report, output_result)
}

#[cfg(test)]
mod tests;
