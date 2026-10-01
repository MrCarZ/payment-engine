use crate::manager::payment::SourceContext;

/// A complete, ordered source snapshot. Construction alone does not validate it.
#[derive(Debug)]
pub struct Source<R> {
    context: SourceContext,
    records: Vec<R>,
}

impl<R> Source<R> {
    pub fn new(context: SourceContext, records: Vec<R>) -> Self {
        Self { context, records }
    }
    pub fn context(&self) -> &SourceContext {
        &self.context
    }
    pub fn records(&self) -> &[R] {
        &self.records
    }
    pub fn into_parts(self) -> (SourceContext, Vec<R>) {
        (self.context, self.records)
    }
}
