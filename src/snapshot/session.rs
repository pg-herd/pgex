use std::{num::NonZeroUsize, sync::Arc};

use anyhow::{Context, Result};
use tokio::sync::{Semaphore, watch};

use crate::connection::{Connector, Session};

use super::{
    error::SnapshotScopeError,
    lease::SnapshotOptions,
    transaction::{SnapshotTimeouts, SourceReadMode, begin_read_transaction},
};

pub struct SnapshotReaders<'scope> {
    sessions: Arc<SnapshotReadSessions>,
    exporter_disconnected: &'scope watch::Sender<()>,
}

pub(crate) struct SnapshotReadSessions {
    source: Connector,
    key: Option<String>,
    client_connection_check_interval_ms: u32,
    connection_limit: NonZeroUsize,
    reader_slots: Semaphore,
    row_security_off: bool,
    timeouts: SnapshotTimeouts,
}

impl<'scope> SnapshotReaders<'scope> {
    pub(super) fn new(
        source: Connector,
        key: Option<String>,
        options: &SnapshotOptions,
        exporter_disconnected: &'scope watch::Sender<()>,
    ) -> Self {
        Self {
            sessions: Arc::new(SnapshotReadSessions {
                source,
                key,
                client_connection_check_interval_ms: options.client_connection_check_interval_ms,
                connection_limit: options.reader_connection_limit,
                reader_slots: Semaphore::new(options.reader_connection_limit.get()),
                row_security_off: options.row_security_off,
                timeouts: options.reader_timeouts,
            }),
            exporter_disconnected,
        }
    }

    /// Maximum number of concurrent snapshot reader sessions.
    pub fn connection_limit(&self) -> NonZeroUsize {
        self.sessions.connection_limit
    }

    pub(crate) fn sessions(&self) -> Arc<SnapshotReadSessions> {
        Arc::clone(&self.sessions)
    }

    /// The subscriber must stop its work and release its sessions when the exporter disconnects.
    pub(crate) fn watch_exporter_disconnect(&self) -> watch::Receiver<()> {
        self.exporter_disconnected.subscribe()
    }

    /// Opens a source reader in the selected mode and runs the callback.
    pub async fn with_reader<T, E>(
        &self,
        work: impl AsyncFnOnce(&Session) -> Result<T, E>,
    ) -> Result<T, SnapshotScopeError<E>> {
        self.sessions.with_reader(work).await
    }
}

impl SnapshotReadSessions {
    pub(crate) fn row_security_off(&self) -> bool {
        self.row_security_off
    }

    pub(crate) async fn with_reader<T, E>(
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
        let mode = if self.key.is_some() {
            SourceReadMode::SharedSnapshot
        } else {
            SourceReadMode::ReadCommitted
        };
        begin_read_transaction(
            reader,
            self.client_connection_check_interval_ms,
            self.timeouts,
            mode,
        )
        .await
        .map_err(SnapshotScopeError::Setup)?;
        let result = async {
            if let Some(key) = &self.key {
                // PostgreSQL requires a string literal for SET TRANSACTION SNAPSHOT.
                let key_literal = reader
                    .client()
                    .quote_literal(key)
                    .await
                    .context("Quote source snapshot key")
                    .map_err(SnapshotScopeError::Setup)?;
                reader
                    .client()
                    .batch_execute(&format!("SET TRANSACTION SNAPSHOT {key_literal}"))
                    .await
                    .context("Import source snapshot")
                    .map_err(SnapshotScopeError::Setup)?;
            }
            work(reader).await.map_err(SnapshotScopeError::Work)
        }
        .await;
        let Err(error) = reader
            .client()
            .batch_execute("ROLLBACK")
            .await
            .context("Roll back source reader")
        else {
            return result;
        };

        Err(SnapshotScopeError::Rollback {
            error,
            prior_failure: result.err().map(Box::new),
        })
    }
}
