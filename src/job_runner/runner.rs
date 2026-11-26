use anyhow::Result;

use pgex::{
    catalog::CatalogReader,
    snapshot::{SnapshotLease, SnapshotOptions, SourceReadMode},
    table::QualifiedTable,
};

use super::Job;

pub(crate) struct JobRunner {
    catalog: CatalogReader,
    snapshots: SnapshotLease,
}

impl JobRunner {
    pub(crate) fn new(catalog: CatalogReader, snapshots: SnapshotLease) -> Self {
        Self { catalog, snapshots }
    }

    pub(crate) async fn run(
        &self,
        job: &impl Job,
        selected: &[QualifiedTable],
        options: SnapshotOptions,
        read_mode: SourceReadMode,
    ) -> Result<()> {
        let tables = self.table_list(selected).await?;
        self.snapshots
            .with_snapshot(options, read_mode, async |readers| {
                let source_layouts = readers
                    .with_reader(async |session| {
                        self.catalog.read_tables_on(session, &tables).await
                    })
                    .await?;
                job.run(&source_layouts, &readers).await
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
