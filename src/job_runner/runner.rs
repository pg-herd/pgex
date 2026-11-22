use anyhow::Result;

use pgex::{
    catalog::CatalogReader,
    config::compiled::SourceFilterPolicy,
    plan::planner::plan_source_tables,
    snapshot::{SnapshotLease, SnapshotOptions, SourceReadMode},
    table::QualifiedTable,
    targets::Target,
};

pub(crate) struct JobRunner<T> {
    catalog: CatalogReader,
    snapshots: SnapshotLease,
    target: T,
}

impl<T: Target> JobRunner<T> {
    pub(crate) fn new(catalog: CatalogReader, snapshots: SnapshotLease, target: T) -> Self {
        Self {
            catalog,
            snapshots,
            target,
        }
    }

    pub(crate) async fn run(
        &self,
        options: SnapshotOptions<'_>,
        read_mode: SourceReadMode,
        filters: &SourceFilterPolicy,
    ) -> Result<()> {
        let tables = self.table_list(options.tables).await?;
        let options = SnapshotOptions {
            tables: &tables,
            ..options
        };
        self.snapshots
            .with_readers(options, read_mode, async |readers| {
                let source_layouts = readers
                    .with_reader(async |session| {
                        self.catalog.read_tables_on(session, &tables).await
                    })
                    .await?;
                let source_plan = plan_source_tables(&source_layouts, filters)?;
                self.target.sync_rows(&readers, &source_plan).await
            })
            .await?;
        Ok(())
    }

    async fn table_list(&self, supplied: &[QualifiedTable]) -> Result<Vec<QualifiedTable>> {
        if supplied.is_empty() {
            return self.catalog.list_tables().await;
        }
        Ok(supplied.to_vec())
    }
}
