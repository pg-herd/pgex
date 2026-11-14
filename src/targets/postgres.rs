mod commit_group;
mod predicate;
mod transfer;
mod worker;

use std::num::{NonZeroU32, NonZeroUsize};

use anyhow::{Context, Result};
use tokio::io::{self, AsyncWriteExt};

use crate::{
    catalog::{CatalogReader, TableLayout, validate_layouts},
    config::compiled::TableFilter,
    connection::{Connector, Session},
    plan::planner::{SourcePlan, index_table_filters},
    predicate::Predicate,
    snapshot::SnapshotReaders,
    table::{QualifiedTable, with_locked_tables},
    work_queue::WorkQueue,
};

use super::Target;

pub const DEFAULT_PARALLEL_TRANSFERS: NonZeroUsize = NonZeroUsize::new(8).unwrap();
pub const DEFAULT_COMMIT_GROUP_MIB: NonZeroU32 = NonZeroU32::new(32).unwrap();

pub struct PostgresTargetOptions {
    pub validate_layouts: bool,
    pub workers: NonZeroUsize,
    pub commit_group_mib: NonZeroU32,
    pub replica_role: bool,
    pub row_security_off: bool,
    pub skip_delete: bool,
    pub delete_filters: Vec<TableFilter>,
}

struct PostgresTable<'a> {
    layout: &'a TableLayout,
    copy_filter: Option<&'a Predicate>,
    delete_filter: Option<&'a Predicate>,
}

pub struct PostgresTarget {
    connector: Connector,
    catalog: CatalogReader,
    options: PostgresTargetOptions,
}

impl PostgresTarget {
    pub fn new(connector: Connector, options: PostgresTargetOptions) -> Self {
        Self {
            catalog: CatalogReader::new(connector.clone()),
            connector,
            options,
        }
    }

    async fn validate_on(
        &self,
        session: &Session,
        tables: &[QualifiedTable],
        source_plan: &SourcePlan<'_>,
    ) -> Result<()> {
        let target = self
            .catalog
            .read_tables_on(session, tables)
            .await
            .context("Read destination table layouts")?;
        validate_layouts(
            source_plan.tables.iter().map(|table| table.layout),
            &target.tables,
        )
    }
}

impl Target for PostgresTarget {
    async fn run(&self, readers: &SnapshotReaders<'_>, source_plan: &SourcePlan<'_>) -> Result<()> {
        let tables: Vec<_> = source_plan
            .tables
            .iter()
            .map(|table| table.layout.name.clone())
            .collect();
        let delete_filters = index_table_filters(
            &tables,
            &self.options.delete_filters,
            "target.delete_filters",
        )?;
        let uses_filters = !delete_filters.is_empty()
            || source_plan
                .tables
                .iter()
                .any(|table| table.filter.is_some());
        let parallel_transfers = self.options.workers;
        let reader_limit = readers.connection_limit();
        let transfer_limit = parallel_transfers.min(reader_limit);
        if parallel_transfers > reader_limit {
            let warning = format!(
                "Warning: PostgreSQL target parallel transfers ({parallel_transfers}) exceed the source reader connection limit ({reader_limit}). Capping the worker count at {transfer_limit}.\n"
            );
            let mut stderr = io::stderr();
            let _ = stderr.write_all(warning.as_bytes()).await;
            let _ = stderr.flush().await;
        }

        with_locked_tables(&self.connector, &tables, async |session| {
            if self.options.validate_layouts {
                self.validate_on(session, &tables, source_plan).await?;
            }
            let queue = WorkQueue::new(source_plan.tables.iter().map(|table| {
                PostgresTable {
                    layout: table.layout,
                    copy_filter: table.filter,
                    delete_filter: delete_filters
                        .get(&table.layout.name)
                        .copied()
                        .or(table.filter),
                }
            }));
            let worker_count = transfer_limit.get().min(source_plan.tables.len());
            worker::run_workers(worker_count, async |worker_id| {
                worker::run_worker(
                    &self.connector,
                    readers,
                    &queue,
                    uses_filters,
                    &self.options,
                )
                .await
                .with_context(|| format!("Run PostgreSQL transfer worker {worker_id}"))
            })
            .await
        })
        .await
        .context("Run with destination tables locked")
    }
}
