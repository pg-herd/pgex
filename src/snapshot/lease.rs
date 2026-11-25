use std::num::NonZeroUsize;

use anyhow::{Context, Result, ensure};
use tokio::sync::{Semaphore, watch};

use crate::connection::{Connector, Session};

use super::{
    error::SnapshotScopeError,
    export::export_snapshot,
    session::SnapshotReaders,
    transaction::{SnapshotTimeouts, SourceReadMode, begin_read_transaction},
};

pub const DEFAULT_CLIENT_CONNECTION_CHECK_INTERVAL_MS: u32 = 1_000;
pub const DEFAULT_READER_CONNECTION_LIMIT: NonZeroUsize = NonZeroUsize::new(8).unwrap();

pub struct SnapshotOptions {
    pub client_connection_check_interval_ms: u32,
    pub reader_connection_limit: NonZeroUsize,
    pub row_security_off: bool,
    pub exporter_timeouts: SnapshotTimeouts,
    pub reader_timeouts: SnapshotTimeouts,
}

impl Default for SnapshotOptions {
    fn default() -> Self {
        Self {
            client_connection_check_interval_ms: DEFAULT_CLIENT_CONNECTION_CHECK_INTERVAL_MS,
            reader_connection_limit: DEFAULT_READER_CONNECTION_LIMIT,
            row_security_off: false,
            exporter_timeouts: SnapshotTimeouts::default(),
            reader_timeouts: SnapshotTimeouts::default(),
        }
    }
}

pub struct SnapshotLease {
    source: Connector,
}

impl SnapshotLease {
    pub fn new(source: Connector) -> Self {
        Self { source }
    }

    /// Holds an exporting transaction and its session for the duration of the callback.
    /// Stops the callback if the exporter disconnects.
    pub async fn with_snapshot<T, E>(
        &self,
        options: SnapshotOptions,
        work: impl for<'scope> AsyncFnOnce(SnapshotReaders<'scope>) -> Result<T, E>,
    ) -> Result<T, SnapshotScopeError<E>> {
        self.with_readers(options, SourceReadMode::SharedSnapshot, work)
            .await
    }

    /// Keeps the source coordinator session open while the callback opens readers in the selected mode.
    pub async fn with_readers<T, E>(
        &self,
        options: SnapshotOptions,
        mode: SourceReadMode,
        work: impl for<'scope> AsyncFnOnce(SnapshotReaders<'scope>) -> Result<T, E>,
    ) -> Result<T, SnapshotScopeError<E>> {
        validate_reader_connection_limit(options.reader_connection_limit)
            .map_err(SnapshotScopeError::Setup)?;
        let mut exporter = self
            .source
            .connect()
            .await
            .map_err(SnapshotScopeError::Setup)?;
        let result = self.with_exporter(&mut exporter, options, mode, work).await;
        exporter.close().await;
        result
    }

    async fn with_exporter<T, E>(
        &self,
        exporter: &mut Session,
        options: SnapshotOptions,
        mode: SourceReadMode,
        work: impl for<'scope> AsyncFnOnce(SnapshotReaders<'scope>) -> Result<T, E>,
    ) -> Result<T, SnapshotScopeError<E>> {
        begin_read_transaction(
            exporter,
            options.client_connection_check_interval_ms,
            options.exporter_timeouts,
            mode,
        )
        .await
        .map_err(SnapshotScopeError::Setup)?;
        let result = async {
            let key = match mode {
                SourceReadMode::SharedSnapshot => Some(
                    export_snapshot(exporter)
                        .await
                        .map_err(SnapshotScopeError::Setup)?,
                ),
                SourceReadMode::ReadCommitted => None,
            };
            let (exporter_disconnected, _) = watch::channel(());
            let readers =
                SnapshotReaders::new(self.source.clone(), key, &options, &exporter_disconnected);
            let work = work(readers);
            tokio::pin!(work);
            tokio::select! {
                biased;
                error = exporter.client().wait_for_disconnect() => {
                    // Let the worker supervisor join its tasks while it still holds the target locks.
                    if exporter_disconnected.send(()).is_ok() {
                        let _ = work.await;
                    }
                    Err(SnapshotScopeError::ExporterLost(error))
                }
                result = &mut work => result.map_err(SnapshotScopeError::Work),
            }
        }
        .await;

        if matches!(result, Err(SnapshotScopeError::ExporterLost(_))) {
            return result;
        }

        let Err(error) = exporter
            .client()
            .batch_execute("ROLLBACK")
            .await
            .context("Roll back source coordinator transaction")
        else {
            return result;
        };

        Err(SnapshotScopeError::Rollback {
            error,
            prior_failure: result.err().map(Box::new),
        })
    }
}

fn validate_reader_connection_limit(limit: NonZeroUsize) -> Result<()> {
    ensure!(
        limit.get() <= Semaphore::MAX_PERMITS,
        "Source reader connection limit exceeds {}",
        Semaphore::MAX_PERMITS
    );
    Ok(())
}
