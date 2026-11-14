use std::num::NonZeroUsize;

use anyhow::{Context, Result};
use postgres_protocol::escape::escape_literal;
use tokio::sync::Semaphore;

use crate::connection::{Connector, Session};

use super::{
    error::SnapshotScopeError,
    lease::SnapshotOptions,
    transaction::{SnapshotTimeouts, begin_read_transaction},
};

pub struct SnapshotReaders<'scope> {
    source: &'scope Connector,
    key: &'scope str,
    client_connection_check_interval_ms: u32,
    connection_limit: NonZeroUsize,
    reader_slots: Semaphore,
    row_security_off: bool,
    timeouts: SnapshotTimeouts,
}

impl<'scope> SnapshotReaders<'scope> {
    pub(super) fn new(
        source: &'scope Connector,
        key: &'scope str,
        options: &SnapshotOptions<'_>,
    ) -> Self {
        Self {
            source,
            key,
            client_connection_check_interval_ms: options.client_connection_check_interval_ms,
            connection_limit: options.reader_connection_limit,
            reader_slots: Semaphore::new(options.reader_connection_limit.get()),
            row_security_off: options.row_security_off,
            timeouts: options.reader_timeouts,
        }
    }

    /// Maximum number of concurrent snapshot reader sessions.
    pub fn connection_limit(&self) -> NonZeroUsize {
        self.connection_limit
    }

    pub(crate) fn row_security_off(&self) -> bool {
        self.row_security_off
    }

    /// Waits for a reader slot, imports the snapshot on a new session, and runs the callback.
    pub async fn with_reader<T, E>(
        &self,
        work: impl AsyncFnOnce(&Session) -> Result<T, E>,
    ) -> Result<T, SnapshotScopeError<E>> {
        // Hold the permit until the reader session is closed.
        let _reader_permit = self
            .reader_slots
            .acquire()
            .await
            .context("Acquire source reader slot")
            .map_err(SnapshotScopeError::Setup)?;
        let reader = self
            .source
            .connect()
            .await
            .map_err(SnapshotScopeError::Setup)?;
        let result = self.read_on(&reader, work).await;
        reader.close().await;
        result
    }

    async fn read_on<T, E>(
        &self,
        reader: &Session,
        work: impl AsyncFnOnce(&Session) -> Result<T, E>,
    ) -> Result<T, SnapshotScopeError<E>> {
        begin_read_transaction(
            reader,
            self.client_connection_check_interval_ms,
            self.timeouts,
        )
        .await
        .map_err(SnapshotScopeError::Setup)?;
        let result = async {
            // PostgreSQL requires a string literal for SET TRANSACTION SNAPSHOT.
            let key_literal = escape_literal(self.key);
            reader
                .client()
                .batch_execute(&format!("SET TRANSACTION SNAPSHOT {key_literal}"))
                .await
                .context("Import source snapshot")
                .map_err(SnapshotScopeError::Setup)?;
            work(reader).await.map_err(SnapshotScopeError::Work)
        }
        .await;
        let Err(error) = reader
            .client()
            .batch_execute("ROLLBACK")
            .await
            .context("Roll back snapshot reader")
        else {
            return result;
        };

        Err(SnapshotScopeError::Rollback {
            error,
            prior_failure: result.err().map(Box::new),
        })
    }
}
