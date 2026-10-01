use std::error::Error;

use crate::domain::payment::{Account, ClientId};

/// Publishes borrowed account snapshots and completes delivery before returning.
/// Implementations choose representation, destination, sorting, and flushing.
pub trait Output {
    type Error: Error;

    fn publish<'a>(
        &mut self,
        accounts: impl IntoIterator<Item = (ClientId, &'a Account)>,
    ) -> Result<(), Self::Error>;
}
