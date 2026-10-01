use std::{io::Read, iter::FusedIterator, sync::Arc};

use csv::{Position as CsvPosition, Reader, ReaderBuilder, StringRecord, Trim};

use crate::{
    domain::payment::{LifecycleAction, transaction::Type as TransactionType},
    manager::payment::{Context, RecordPosition, Request, SourceContext},
};

mod error;

pub use error::{FieldError, InputError, Type};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub request: Request,
    pub context: Context,
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
                context: input.context(error.position().unwrap_or(input.reader.position())),
                error_type: Type::Csv(error),
            })?;
        let expected = ["type", "client", "tx", "amount"];
        if headers.len() != expected.len()
            || expected
                .iter()
                .any(|name| headers.iter().filter(|field| field == name).count() != 1)
        {
            return Err(InputError {
                context: input.context(headers.position().unwrap_or(input.reader.position())),
                error_type: Type::InvalidHeaders,
            });
        }
        for (index, name) in expected.iter().enumerate() {
            // Header validation ensures that each name occurs exactly once.
            input.columns[index] = headers.iter().position(|field| field == *name).unwrap();
        }
        Ok(input)
    }

    fn context(&self, position: &CsvPosition) -> Context {
        Context {
            source: Arc::clone(&self.source),
            position: RecordPosition {
                record: position.record(),
                line: position.line(),
                byte: position.byte(),
            },
        }
    }

    fn parse(&self) -> Result<Request, Type> {
        // Missing final amount is accepted for lifecycle rows. Original rows
        // subsequently fail amount validation; other short/long rows are invalid.
        if self.record.len() != 4 && !(self.record.len() == 3 && self.columns[3] == 3) {
            return Err(Type::InvalidRecordLength);
        }
        let [event, client, tx, amount] = self
            .columns
            .map(|index| self.record.get(index).unwrap_or(""));
        let client = client.parse().map_err(|error| Type::InvalidField {
            field: "client",
            error: FieldError::Identifier(error),
        })?;
        let tx = tx.parse().map_err(|error| Type::InvalidField {
            field: "tx",
            error: FieldError::Identifier(error),
        })?;
        match event {
            "deposit" | "withdrawal" => {
                if amount.is_empty() {
                    return Err(Type::InvalidField {
                        field: "amount",
                        error: FieldError::MissingAmount,
                    });
                }
                let amount = amount.parse().map_err(|error| Type::InvalidField {
                    field: "amount",
                    error: FieldError::Amount(error),
                })?;
                let transaction_type = if event == "deposit" {
                    TransactionType::Deposit
                } else {
                    TransactionType::Withdrawal
                };
                Ok(Request::Original {
                    client,
                    tx,
                    transaction_type,
                    amount,
                })
            }
            "dispute" | "resolve" | "chargeback" => {
                let action = match event {
                    "dispute" => LifecycleAction::Dispute,
                    "resolve" => LifecycleAction::Resolve,
                    _ => LifecycleAction::Chargeback,
                };
                Ok(Request::Lifecycle { client, tx, action })
            }
            _ => Err(Type::InvalidField {
                field: "type",
                error: FieldError::UnknownType,
            }),
        }
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
                let context =
                    self.context(self.record.position().unwrap_or(self.reader.position()));
                match self.parse() {
                    Ok(request) => Some(Ok(Record { request, context })),
                    Err(error_type) => {
                        self.finished = true;
                        Some(Err(InputError {
                            context,
                            error_type,
                        }))
                    }
                }
            }
            Err(error) => {
                self.finished = true;
                Some(Err(InputError {
                    context: self.context(error.position().unwrap_or(self.reader.position())),
                    error_type: Type::Csv(error),
                }))
            }
        }
    }
}

impl<R: Read> FusedIterator for Input<R> {}

#[cfg(test)]
mod tests;
