use std::io::Write;

use csv::WriterBuilder;
use serde::Serialize;

use crate::{
    domain::payment::{Account, ClientId},
    manager::payment::run::Output as AccountOutput,
};

mod error;

pub use error::OutputError;

/// External account representation with exact four-place decimal formatting.
#[derive(Debug, Serialize)]
struct Row {
    client: u16,
    available: String,
    held: String,
    total: String,
    locked: bool,
}

impl TryFrom<(ClientId, &Account)> for Row {
    type Error = OutputError;

    fn try_from((client, account): (ClientId, &Account)) -> Result<Self, Self::Error> {
        Ok(Self {
            client: client.get(),
            available: account.available().to_string(),
            held: account.held().to_string(),
            total: account.total()?.to_string(),
            locked: account.is_locked(),
        })
    }
}

/// Writes a complete account report, sorted by client ID, and flushes the writer.
///
/// Collects account references for sorting, not transaction history. The header
/// is emitted even for an empty account set. Write failures can leave partial
/// output; the caller owns any atomic file-publication requirement.
pub fn write<'a>(
    output: impl Write,
    accounts: impl IntoIterator<Item = (ClientId, &'a Account)>,
) -> Result<(), OutputError> {
    let mut accounts: Vec<_> = accounts.into_iter().collect();
    accounts.sort_unstable_by_key(|(client, _)| *client);

    let mut writer = WriterBuilder::new().has_headers(false).from_writer(output);
    writer.write_record(["client", "available", "held", "total", "locked"])?;
    for account in accounts {
        writer.serialize(Row::try_from(account)?)?;
    }
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests;

/// CSV implementation of the manager's account publication contract.
pub struct Output<W>(W);
impl<W> Output<W> {
    pub fn new(writer: W) -> Self {
        Self(writer)
    }
}
impl<W: Write> AccountOutput for Output<W> {
    type Error = OutputError;
    fn publish<'a>(
        &mut self,
        accounts: impl IntoIterator<Item = (ClientId, &'a Account)>,
    ) -> Result<(), Self::Error> {
        write(&mut self.0, accounts)
    }
}
