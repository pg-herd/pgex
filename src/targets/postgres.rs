mod commit_group;
mod predicate;
mod transfer;
mod truncate;
mod worker;

use std::{
    num::{NonZeroU32, NonZeroUsize},
    sync::Arc,
};

use anyhow::{Context, Result};
use tokio::io::{self, AsyncWriteExt};

use crate::{
    catalog::{CatalogReader, validate_layouts},
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
    pub no_locks: bool,
    pub workers: NonZeroUsize,
    pub commit_group_mib: NonZeroU32,
    pub replica_role: bool,
    pub row_security_off: bool,
    pub skip_delete: bool,
    pub truncate: bool,
    pub delete_filters: Vec<TableFilter>,
}

struct PostgresTable {
    name: QualifiedTable,
    columns: Vec<String>,
    copy_filter: Option<Predicate>,
    delete_filter: Option<Predicate>,
}

pub struct PostgresTarget {
    connector: Connector,
    catalog: CatalogReader,
    options: Arc<PostgresTargetOptions>,
}

impl PostgresTarget {
    pub fn new(connector: Connector, options: PostgresTargetOptions) -> Self {
        Self {
            catalog: CatalogReader::new(connector.clone()),
            connector,
            options: Arc::new(options),
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
    async fn sync_rows(
        &self,
        readers: &SnapshotReaders<'_>,
        source_plan: &SourcePlan<'_>,
    ) -> Result<()> {
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

        let transfer = async || {
            let queue = Arc::new(WorkQueue::new(source_plan.tables.iter().map(|table| {
                PostgresTable {
                    name: table.layout.name.clone(),
                    columns: table
                        .layout
                        .columns
                        .iter()
                        .filter(|column| !column.generated)
                        .map(|column| column.name.clone())
                        .collect(),
                    copy_filter: table.filter.cloned(),
                    delete_filter: delete_filters
                        .get(&table.layout.name)
                        .copied()
                        .or(table.filter)
                        .cloned(),
                }
            })));
            let worker_count = transfer_limit.get().min(source_plan.tables.len());
            let source_sessions = readers.sessions();
            worker::run_workers(
                worker_count,
                readers.watch_exporter_disconnect(),
                |worker_id| {
                    let connector = self.connector.clone();
                    let source_sessions = Arc::clone(&source_sessions);
                    let queue = Arc::clone(&queue);
                    let options = Arc::clone(&self.options);
                    async move {
                        worker::run_worker(
                            &connector,
                            &source_sessions,
                            &queue,
                            uses_filters,
                            &options,
                        )
                        .await
                        .with_context(|| format!("Run PostgreSQL transfer worker {worker_id}"))
                    }
                },
            )
            .await
        };

        if self.options.no_locks || self.options.truncate {
            if self.options.validate_layouts {
                self.connector
                    .with_connection(async |session| {
                        self.validate_on(session, &tables, source_plan).await
                    })
                    .await?;
            }
            return transfer().await;
        }

        with_locked_tables(&self.connector, &tables, async |session| {
            if self.options.validate_layouts {
                self.validate_on(session, &tables, source_plan).await?;
            }
            transfer().await
        })
        .await
        .context("Run with destination tables locked")
    }
}
