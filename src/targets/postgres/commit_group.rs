use anyhow::{Context, Result};

use crate::{connection::Session, work_queue::WorkQueue};

use super::{
    PostgresTable, PostgresTargetOptions,
    transfer::{copy_table, delete_rows, lock_source_for_copy, set_replica_role_if_enabled},
};

enum GroupEnd {
    LimitReached,
    QueueDrained,
}

pub(super) async fn transfer_groups(
    source: &Session,
    destination: &mut Session,
    queue: &WorkQueue<PostgresTable>,
    options: &PostgresTargetOptions,
) -> Result<()> {
    while let Some(first_table) = queue.pop_front().await {
        let end = destination
            .with_transaction(async |destination| {
                set_replica_role_if_enabled(destination, options.replica_role).await?;
                transfer_group(source, destination, queue, first_table, options).await
            })
            .await?;
        if let GroupEnd::QueueDrained = end {
            break;
        }
    }
    Ok(())
}

async fn transfer_group(
    source: &Session,
    destination: &Session,
    queue: &WorkQueue<PostgresTable>,
    first_table: PostgresTable,
    options: &PostgresTargetOptions,
) -> Result<GroupEnd> {
    let limit_bytes = u64::from(options.commit_group_mib.get()) * 1024 * 1024;
    let mut accumulated_bytes = 0u64;
    let mut table = first_table;
    loop {
        let write = async {
            lock_source_for_copy(source, &table.name).await?;
            if !options.skip_delete {
                delete_rows(destination, &table).await?;
            }
            copy_table(source, destination, &table).await
        }
        .await
        .with_context(|| format!("Transfer table {}", table.name))?;
        accumulated_bytes = accumulated_bytes.saturating_add(write.payload_bytes);
        if accumulated_bytes >= limit_bytes {
            return Ok(GroupEnd::LimitReached);
        }

        let Some(next_table) = queue.pop_front().await else {
            return Ok(GroupEnd::QueueDrained);
        };
        table = next_table;
    }
}
