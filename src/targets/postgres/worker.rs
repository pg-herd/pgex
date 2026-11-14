use std::future::Future;

use anyhow::{Context, Result};
use futures_util::stream::{self, TryStreamExt};

use crate::{connection::Connector, snapshot::SnapshotReaders, work_queue::WorkQueue};

use super::{PostgresTable, PostgresTargetOptions, commit_group::transfer_groups};

pub(super) async fn run_workers<F>(
    worker_count: usize,
    run_worker: impl FnMut(usize) -> F,
) -> Result<()>
where
    F: Future<Output = Result<()>>,
{
    stream::iter((1..=worker_count).map(Ok))
        .try_for_each_concurrent(None, run_worker)
        .await
}

pub(super) async fn run_worker(
    connector: &Connector,
    readers: &SnapshotReaders<'_>,
    queue: &WorkQueue<PostgresTable<'_>>,
    uses_filters: bool,
    options: &PostgresTargetOptions,
) -> Result<()> {
    readers
        .with_reader(async |source| {
            if uses_filters {
                source
                    .client()
                    .batch_execute("SET LOCAL standard_conforming_strings = on")
                    .await
                    .context("Set PostgreSQL string syntax for source filters")?;
            }
            if readers.row_security_off() {
                source
                    .client()
                    .batch_execute("SET LOCAL row_security = off")
                    .await
                    .context("Set row_security to off for source reader")?;
            }
            connector
                .with_connection(async |destination| {
                    if uses_filters {
                        destination
                            .client()
                            .batch_execute("SET standard_conforming_strings = on")
                            .await
                            .context("Set PostgreSQL string syntax for destination filters")?;
                    }
                    if options.row_security_off {
                        destination
                            .client()
                            .batch_execute("SET row_security = off")
                            .await
                            .context("Set row_security to off for destination worker")?;
                    }
                    transfer_groups(source, destination, queue, options).await
                })
                .await
                .context("Run transfers on destination session")
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
