//! Transport-independent sequential payment coordination and run accounting.

use std::error::Error;

use crate::{domain::observability::Event, manager::observability::TraceService};

use super::{
    Context, Outcome, PaymentManager, ProcessingError, Report, Request,
    trace::{processing_failed, request_processed},
};

mod error;
pub use error::Failure;

/// Applied/ignored/rejected exclude replays. Replayed counts all original
/// retries regardless of their stored outcome.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub applied: u64,
    pub ignored: u64,
    pub rejected: u64,
    pub replayed: u64,
    pub input_errors: u64,
    pub processing_errors: u64,
}

/// Validated request envelope. Adapters may enrich its trace events with
/// transport provenance without exposing transport types to the coordinator.
pub trait Record {
    fn request(&self) -> Request;
    fn context(&self) -> &Context;

    fn processed_event(&self, report: Report) -> Event {
        request_processed(self.context(), self.request(), report)
    }

    fn failed_event(&self, error: ProcessingError) -> Event {
        processing_failed(self.context(), self.request(), error)
    }
}

pub trait InputFailure: Error {
    fn event(&self) -> Event;
}

#[derive(Debug, Default)]
pub struct Coordinator {
    manager: PaymentManager,
    summary: Summary,
}

impl Coordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn manager(&self) -> &PaymentManager {
        &self.manager
    }

    pub const fn summary(&self) -> Summary {
        self.summary
    }

    /// Stops on the first input, processing, or trace failure. Delivery never
    /// retries payment processing. Business outcomes continue in input order.
    pub fn process<R: Record, E: InputFailure>(
        &mut self,
        records: impl IntoIterator<Item = Result<R, E>>,
        trace: &mut dyn TraceService,
    ) -> Result<(), Failure<R, E>> {
        for record in records {
            let record = match record {
                Ok(record) => record,
                Err(error) => {
                    self.summary.input_errors += 1;
                    let trace_error = trace.emit(error.event()).err();
                    return Err(Failure::Input { error, trace_error });
                }
            };
            let report = match self.manager.process(record.request()) {
                Ok(report) => report,
                Err(error) => {
                    self.summary.processing_errors += 1;
                    let trace_error = trace.emit(record.failed_event(error)).err();
                    return Err(Failure::Processing {
                        record,
                        error,
                        trace_error,
                    });
                }
            };
            if report.is_replay() {
                self.summary.replayed += 1;
            } else {
                match report.outcome() {
                    Outcome::Applied => self.summary.applied += 1,
                    Outcome::Ignored(_) => self.summary.ignored += 1,
                    Outcome::Rejected(_) => self.summary.rejected += 1,
                }
            }
            trace
                .emit(record.processed_event(report))
                .map_err(Failure::Trace)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
