use crate::connection::{CopyIn, PgConnection};
use anyhow::{Context, Result};
use bytes::{Buf, Bytes, BytesMut};

use crate::connection::Session;
use crate::table::QualifiedTable;

use super::{
    PostgresTable,
    predicate::{bind_value, copy_condition, where_clause},
};

const COPY_BUFFER_BYTES: usize = 256 * 1024;

pub(super) struct TableWriteResult {
    /// Encoded payload accepted by the destination.
    pub(super) payload_bytes: u64,
}

pub(super) async fn lock_source_for_copy(source: &Session, table: &QualifiedTable) -> Result<()> {
    let relation = source
        .client()
        .quote_qualified_identifier(table.schema(), table.table())
        .await?;
    // Match COPY's SELECT FROM relation, including inheritance and partition children.
    source
        .client()
        .batch_execute(&format!(
            "LOCK TABLE {relation} IN ACCESS SHARE MODE NOWAIT"
        ))
        .await
        .with_context(|| format!("Acquire source COPY lock on {relation} without waiting"))
}

pub(super) async fn copy_table(
    source: &Session,
    destination: &Session,
    table: &PostgresTable,
) -> Result<TableWriteResult> {
    let mut destination_guard = destination.client().connection().await?;
    let destination = destination_guard.get().await?;
    let mut source_guard = source.client().connection().await?;
    let source = source_guard.get().await?;
    let source_relation =
        source.quote_qualified_identifier(table.name.schema(), table.name.table())?;
    let target_relation =
        destination.quote_qualified_identifier(table.name.schema(), table.name.table())?;
    let source_columns = table
        .columns
        .iter()
        .map(|column| source.quote_identifier(column))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let target_columns = table
        .columns
        .iter()
        .map(|column| destination.quote_identifier(column))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let copy_filter = table
        .copy_filter
        .as_ref()
        .map(|predicate| copy_condition(predicate, source))
        .transpose()?
        .map_or_else(String::new, |condition| where_clause(&condition));
    let copy_out = format!(
        "COPY (SELECT {source_columns} FROM {source_relation}{copy_filter}) TO STDOUT (FORMAT TEXT)"
    );
    let copy_columns = if target_columns.is_empty() {
        String::new()
    } else {
        format!(" ({target_columns})")
    };
    let copy_in = format!("COPY {target_relation}{copy_columns} FROM STDIN (FORMAT TEXT)");
    copy_rows(source, destination, &copy_out, &copy_in).await
}

pub(super) async fn delete_rows(destination: &Session, table: &PostgresTable) -> Result<()> {
    let relation = destination
        .client()
        .quote_qualified_identifier(table.name.schema(), table.name.table())
        .await?;
    let mut sql = format!("DELETE FROM {relation}");
    let mut binds = Vec::new();
    if let Some(predicate) = &table.delete_filter {
        sql.push_str(&where_clause(&predicate.sql));
        binds.extend(
            predicate
                .parameters
                .iter()
                .map(|parameter| bind_value(&parameter.value)),
        );
    }
    destination
        .client()
        .execute(&sql, &binds)
        .await
        .context("Delete destination rows")?;
    Ok(())
}

pub(super) async fn set_replica_role_if_enabled(
    destination: &Session,
    replica_role: bool,
) -> Result<()> {
    if replica_role {
        destination
            .client()
            .batch_execute("SET LOCAL session_replication_role = replica")
            .await
            .context("Set destination replication role for the COPY transaction")?;
    }
    Ok(())
}

async fn copy_rows(
    source: &mut PgConnection,
    destination: &mut PgConnection,
    copy_out: &str,
    copy_in: &str,
) -> Result<TableWriteResult> {
    let output = destination
        .copy_in_raw(copy_in)
        .await
        .context("Start destination COPY")?;
    let mut input = match source.copy_out_raw(copy_out).await {
        Ok(input) => input,
        Err(error) => {
            let failure = anyhow::Error::new(error).context("Start source COPY");
            return match output.abort("Source COPY failed to start").await {
                Ok(()) => Err(failure),
                Err(abort) => {
                    Err(failure.context(format!("Abort destination COPY also failed: {abort}")))
                }
            };
        }
    };
    let mut writer = DestinationCopyWriter {
        sink: output,
        payload_bytes: 0,
    };

    let copied: Result<()> = async {
        let mut buffer = BytesMut::with_capacity(COPY_BUFFER_BYTES);
        while let Some(mut chunk) = input.next_chunk().await.context("Read source COPY")? {
            if chunk.len() < COPY_BUFFER_BYTES - buffer.len() {
                buffer.extend_from_slice(&chunk);
                continue;
            }
            while !chunk.is_empty() {
                let length = chunk.len().min(COPY_BUFFER_BYTES - buffer.len());
                buffer.extend_from_slice(&chunk[..length]);
                chunk.advance(length);
                if buffer.len() == COPY_BUFFER_BYTES {
                    writer.write(buffer.split().freeze()).await?;
                    buffer.reserve(COPY_BUFFER_BYTES);
                }
            }
        }
        if !buffer.is_empty() {
            writer.write(buffer.freeze()).await?;
        }
        Ok(())
    }
    .await;
    if let Err(failure) = copied {
        return match writer.sink.abort("COPY relay failed").await {
            Ok(()) => Err(failure),
            Err(abort) => {
                Err(failure.context(format!("Abort destination COPY also failed: {abort}")))
            }
        };
    }
    writer.finish().await
}

struct DestinationCopyWriter<'a> {
    sink: CopyIn<'a>,
    payload_bytes: u64,
}

impl DestinationCopyWriter<'_> {
    async fn write(&mut self, payload: Bytes) -> Result<()> {
        let length = payload.len() as u64;
        self.sink
            .send(payload)
            .await
            .context("Write destination COPY")?;
        self.payload_bytes += length;
        Ok(())
    }

    async fn finish(self) -> Result<TableWriteResult> {
        self.sink
            .finish()
            .await
            .context("Finish destination COPY")?;
        Ok(TableWriteResult {
            payload_bytes: self.payload_bytes,
        })
    }
}
