mod postgres;

use std::future::Future;

use anyhow::Result;

use crate::{plan::planner::SourcePlan, snapshot::SnapshotReaders};

pub use postgres::{
    DEFAULT_COMMIT_GROUP_MIB, DEFAULT_PARALLEL_TRANSFERS, PostgresTarget, PostgresTargetOptions,
};

pub trait Target {
    /// Runs the target's configured checks and schedules work for the selected tables.
    /// All target workers must finish or be stopped before returning.
    fn run(
        &self,
        readers: &SnapshotReaders<'_>,
        source_plan: &SourcePlan<'_>,
    ) -> impl Future<Output = Result<()>> + Send;
}
