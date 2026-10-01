use crate::manager::payment::{
    SourceContext,
    run::{RunError, Summary},
    trace::State,
};
use std::convert::Infallible;

#[derive(Debug)]
pub struct SourceReport<R, O> {
    pub source: SourceContext,
    pub summary: Summary,
    pub state: State,
    pub error: Option<RunError<R, Infallible, O>>,
}
#[derive(Debug)]
pub struct Report<R, O> {
    pub summary: Summary,
    pub sources: Vec<SourceReport<R, O>>,
}
impl<R, O> Default for Report<R, O> {
    fn default() -> Self {
        Self {
            summary: Summary::default(),
            sources: Vec::new(),
        }
    }
}

impl<R, O> Report<R, O> {
    pub(super) fn push(&mut self, source: SourceReport<R, O>) {
        self.summary.applied += source.summary.applied;
        self.summary.ignored += source.summary.ignored;
        self.summary.rejected += source.summary.rejected;
        self.summary.replayed += source.summary.replayed;
        self.summary.input_errors += source.summary.input_errors;
        self.summary.processing_errors += source.summary.processing_errors;
        self.sources.push(source);
    }
    pub(super) fn has_failures(&self) -> bool {
        self.sources.iter().any(|source| source.error.is_some())
    }
}
