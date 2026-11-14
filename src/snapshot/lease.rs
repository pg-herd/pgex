use std::num::NonZeroUsize;

use anyhow::{Context, Result, ensure};
use tokio::sync::Semaphore;

use crate::{
    connection::{Connector, Session},
    table::{QualifiedTable, lock_for_read},
};

use super::{
    error::SnapshotScopeError,
    export::export_snapshot,
    session::SnapshotReaders,
    transaction::{SnapshotTimeouts, begin_read_transaction},
};

pub const DEFAULT_CLIENT_CONNECTION_CHECK_INTERVAL_MS: u32 = 1_000;
pub const DEFAULT_READER_CONNECTION_LIMIT: NonZeroUsize = NonZeroUsize::new(8).unwrap();

pub struct SnapshotOptions<'a> {
    pub tables: &'a [QualifiedTable],
    pub client_connection_check_interval_ms: u32,
    pub reader_connection_limit: NonZeroUsize,
    pub row_security_off: bool,
    pub exporter_timeouts: SnapshotTimeouts,
    pub reader_timeouts: SnapshotTimeouts,
}

impl Default for SnapshotOptions<'_> {
    fn default() -> Self {
        Self {
            tables: &[],
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
    /// Drops the callback if the exporter disconnects.
    pub async fn with_snapshot<T, E>(
        &self,
        options: SnapshotOptions<'_>,
        work: impl for<'scope> AsyncFnOnce(SnapshotReaders<'scope>) -> Result<T, E>,
    ) -> Result<T, SnapshotScopeError<E>> {
        validate_reader_connection_limit(options.reader_connection_limit)
            .map_err(SnapshotScopeError::Setup)?;
        let mut exporter = self
            .source
            .connect()
            .await
            .map_err(SnapshotScopeError::Setup)?;
        let result = self
            .with_exported_snapshot(&mut exporter, options, work)
            .await;
        // The disconnect branch has already joined the driver.
        if !matches!(result, Err(SnapshotScopeError::ExporterLost(_))) {
            exporter.close().await;
        }
        result
    }

    async fn with_exported_snapshot<T, E>(
        &self,
        exporter: &mut Session,
        options: SnapshotOptions<'_>,
        work: impl for<'scope> AsyncFnOnce(SnapshotReaders<'scope>) -> Result<T, E>,
    ) -> Result<T, SnapshotScopeError<E>> {
        begin_read_transaction(
            exporter,
            options.client_connection_check_interval_ms,
            options.exporter_timeouts,
        )
        .await
        .map_err(SnapshotScopeError::Setup)?;
        let result = async {
            lock_for_read(exporter, options.tables)
                .await
                .map_err(SnapshotScopeError::Setup)?;
            let key = export_snapshot(exporter)
                .await
                .map_err(SnapshotScopeError::Setup)?;
            let readers = SnapshotReaders::new(&self.source, &key, &options);
            tokio::select! {
                biased;
                error = exporter.wait_for_disconnect() => Err(SnapshotScopeError::ExporterLost(error)),
                result = work(readers) => result.map_err(SnapshotScopeError::Work),
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
            .context("Roll back snapshot export")
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
