use std::{
    future::Future,
    panic::{AssertUnwindSafe, catch_unwind},
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result, anyhow};
use tokio::{
    runtime::Builder,
    sync::{mpsc, watch},
};

use crate::{connection::Connector, snapshot::SnapshotReadSessions, work_queue::WorkQueue};

use super::{
    PostgresTable, PostgresTargetOptions, commit_group::transfer_groups,
    truncate::truncate_and_copy_tables,
};

pub(super) async fn run_workers<F>(
    worker_count: usize,
    mut exporter_disconnected: watch::Receiver<()>,
    mut run_worker: impl FnMut(usize) -> F,
) -> Result<()>
where
    F: Future<Output = Result<()>> + Send + 'static,
{
    let (finished, mut results) = mpsc::unbounded_channel();
    let (cancel, _) = watch::channel(());
    let mut workers = WorkerThreads {
        cancel,
        handles: Vec::new(),
    };
    for worker_id in 1..=worker_count {
        let finished = finished.clone();
        let cancel = workers.cancel.subscribe();
        let work = run_worker(worker_id);
        let worker = thread::Builder::new()
            .name(format!("pgex-transfer-{worker_id}"))
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| run_on_thread(work, cancel)))
                    .unwrap_or_else(|_| {
                        Err(anyhow!("PostgreSQL transfer worker {worker_id} panicked"))
                    });
                // Report completion after the worker runtime has closed its connection tasks.
                let _ = finished.send(result);
            })
            .with_context(|| format!("Start PostgreSQL transfer worker {worker_id}"))?;
        workers.handles.push(worker);
    }
    drop(finished);

    let result = tokio::select! {
        biased;
        _ = exporter_disconnected.changed() => Err(anyhow!("Source coordinator session disconnected")),
        result = async {
            while let Some(result) = results.recv().await {
                result?;
            }
            Ok(())
        } => result,
    };
    if result.is_err() {
        workers.cancel.send_replace(());
        while results.recv().await.is_some() {}
    }
    result
}

fn run_on_thread(
    work: impl Future<Output = Result<()>>,
    mut cancel: watch::Receiver<()>,
) -> Result<()> {
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Start PostgreSQL transfer worker runtime")?;
    runtime.block_on(async {
        tokio::select! {
            biased;
            _ = cancel.changed() => Ok(()),
            result = work => result,
        }
    })
}

struct WorkerThreads {
    cancel: watch::Sender<()>,
    handles: Vec<JoinHandle<()>>,
}

impl Drop for WorkerThreads {
    fn drop(&mut self) {
        // A dropped supervisor still owns its workers until they finish closing their sessions.
        self.cancel.send_replace(());
        for worker in self.handles.drain(..) {
            let _ = worker.join();
        }
    }
}

pub(super) async fn run_worker(
    connector: &Connector,
    readers: &SnapshotReadSessions,
    queue: &WorkQueue<PostgresTable>,
    uses_filters: bool,
    options: &PostgresTargetOptions,
) -> Result<()> {
    readers
        .with_reader(async |source| {
            // Text COPY must preserve precision and be independent of session locale defaults.
            source
                .client()
                .batch_execute(
                    "SET LOCAL DateStyle = ISO;
                     SET LOCAL IntervalStyle = postgres;
                     SET LOCAL extra_float_digits = 3;
                     SET LOCAL lc_monetary = 'C'",
                )
                .await
                .context("Configure source text COPY serialization")?;
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
                    destination
                        .client()
                        .batch_execute(
                            "SET DateStyle = ISO;
                             SET IntervalStyle = postgres;
                             SET lc_monetary = 'C'",
                        )
                        .await
                        .context("Configure destination text COPY parsing")?;
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
                    if options.truncate {
                        truncate_and_copy_tables(source, destination, queue, options.replica_role)
                            .await
                    } else {
                        transfer_groups(source, destination, queue, options).await
                    }
                })
                .await
                .context("Run transfers on destination session")
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
