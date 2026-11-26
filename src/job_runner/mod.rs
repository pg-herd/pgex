mod runner;
mod sync;

use anyhow::Result;
use pgex::{catalog::Catalog, snapshot::SnapshotReaders};

pub(crate) use runner::JobRunner;
pub(crate) use sync::SyncJob;

pub(crate) trait Job {
    /// Runs with readers that remain available for the duration of the job.
    async fn run(&self, source_layouts: &Catalog, readers: &SnapshotReaders<'_>) -> Result<()>;
}
