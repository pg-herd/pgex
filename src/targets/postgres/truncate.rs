use anyhow::{Context, Result};

use crate::{connection::Session, work_queue::WorkQueue};

use super::{
    PostgresTable,
    transfer::{copy_table, lock_source_for_copy, set_replica_role_if_enabled},
};

pub(super) async fn truncate_and_copy_tables(
    source: &Session,
    destination: &mut Session,
    queue: &WorkQueue<PostgresTable>,
    replica_role: bool,
) -> Result<()> {
    while let Some(table) = queue.pop_front().await {
        lock_source_for_copy(source, &table.name).await?;
        let relation = destination
            .client()
            .quote_qualified_identifier(table.name.schema(), table.name.table())
            .await?;
        destination
            .with_transaction(async |destination| {
                destination
                    .client()
                    .batch_execute(&format!("TRUNCATE {relation} CASCADE"))
                    .await
                    .with_context(|| format!("Truncate destination table {relation}"))?;
                set_replica_role_if_enabled(destination, replica_role).await?;
                copy_table(source, destination, &table).await
            })
            .await
            .with_context(|| format!("Refresh destination table {relation}"))?;
    }
    Ok(())
}
