use std::{io::Read, iter::FusedIterator, sync::Arc};

use csv::{Position as CsvPosition, Reader, ReaderBuilder, StringRecord, Trim};

use crate::manager::payment::{Context, Request, SourceContext};

use super::{RecordPosition, row::Row};

mod error;

pub use error::{FieldError, InputError, Type};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub request: Request,
    pub context: Context,
    pub position: RecordPosition,
}

/// Streaming input with one reusable row buffer. The first failure is yielded
/// once, after which the iterator is exhausted; no rows are skipped on errors.
pub struct Input<R: Read> {
    reader: Reader<R>,
    source: Arc<SourceContext>,
    columns: [usize; 4],
    record: StringRecord,
    finished: bool,
}

impl<R: Read> Input<R> {
    pub fn new(input: R, source: SourceContext) -> Result<Self, InputError> {
        let mut input = Self {
            reader: ReaderBuilder::new()
                .trim(Trim::All)
                .flexible(true)
                .from_reader(input),
            source: Arc::new(source),
            columns: [0; 4],
            record: StringRecord::new(),
            finished: false,
        };
        let headers = input
            .reader
            .headers()
            .cloned()
            .map_err(|error| InputError {
                context: input.context(),
                position: input
                    .record_position(error.position().unwrap_or(input.reader.position())),
                error_type: Type::Csv(error),
            })?;
        let expected = ["type", "client", "tx", "amount"];
        if headers.len() != expected.len()
            || expected
                .iter()
                .any(|name| headers.iter().filter(|field| field == name).count() != 1)
        {
            return Err(InputError {
                context: input.context(),
                position: input
                    .record_position(headers.position().unwrap_or(input.reader.position())),
                error_type: Type::InvalidHeaders,
            });
        }
        for (index, name) in expected.iter().enumerate() {
            // Header validation ensures that each name occurs exactly once.
            input.columns[index] = headers.iter().position(|field| field == *name).unwrap();
        }
        Ok(input)
    }

    fn context(&self) -> Context {
        Context {
            source: Arc::clone(&self.source),
        }
    }

    fn record_position(&self, position: &CsvPosition) -> RecordPosition {
        RecordPosition {
            record: position.record(),
            line: position.line(),
            byte: position.byte(),
        }
    }

    fn parse(&self) -> Result<Request, Type> {
        // Missing final amount is accepted for lifecycle rows. Original rows
        // subsequently fail amount validation; other short/long rows are invalid.
        if self.record.len() != 4 && !(self.record.len() == 3 && self.columns[3] == 3) {
            return Err(Type::InvalidRecordLength);
        }
        let [transaction_type, client, tx, amount] =
            self.columns.map(|index| self.record.get(index));
        let row = Row {
            transaction_type: transaction_type.unwrap_or(""),
            client: client.unwrap_or(""),
            tx: tx.unwrap_or(""),
            amount,
        };
        Request::try_from(row).map_err(|error| Type::InvalidField {
            field: error.field,
            error: error.error,
        })
    }
}

impl<R: Read> Iterator for Input<R> {
    type Item = Result<Record, InputError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        match self.reader.read_record(&mut self.record) {
            Ok(false) => {
                self.finished = true;
                None
            }
            Ok(true) => {
                let context = self.context();
                let position =
                    self.record_position(self.record.position().unwrap_or(self.reader.position()));
                match self.parse() {
                    Ok(request) => Some(Ok(Record {
                        request,
                        context,
                        position,
                    })),
                    Err(error_type) => {
                        self.finished = true;
                        Some(Err(InputError {
                            context,
                            position,
                            error_type,
                        }))
                    }
                }
            }
            Err(error) => {
                self.finished = true;
                Some(Err(InputError {
                    context: self.context(),
                    position: self
                        .record_position(error.position().unwrap_or(self.reader.position())),
                    error_type: Type::Csv(error),
                }))
            }
        }
    }
}

impl<R: Read> FusedIterator for Input<R> {}

#[cfg(test)]
mod tests;
