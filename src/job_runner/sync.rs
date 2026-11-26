use anyhow::Result;
use pgex::{
    catalog::Catalog, config::compiled::SourceFilterPolicy, plan::planner::plan_source_tables,
    snapshot::SnapshotReaders, targets::Target,
};

use super::Job;

pub(crate) struct SyncJob<T> {
    target: T,
    filters: SourceFilterPolicy,
}

impl<T: Target> SyncJob<T> {
    pub(crate) fn new(target: T, filters: SourceFilterPolicy) -> Self {
        Self { target, filters }
    }
}

impl<T: Target> Job for SyncJob<T> {
    async fn run(&self, source_layouts: &Catalog, readers: &SnapshotReaders<'_>) -> Result<()> {
        let source_plan = plan_source_tables(source_layouts, &self.filters)?;
        self.target.sync_rows(readers, &source_plan).await
    }
}
