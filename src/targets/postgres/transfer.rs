use crate::connection::CopyIn;
use anyhow::{Context, Result};
use bytes::{Buf, Bytes, BytesMut};
use futures_util::TryStreamExt;
use postgres_protocol::escape::escape_identifier;

use crate::connection::Session;

use super::{
    PostgresTable,
    predicate::{bind_value, copy_condition, where_clause},
};

const COPY_BUFFER_BYTES: usize = 256 * 1024;

pub(super) struct TableWriteResult {
    /// Encoded payload accepted by the destination.
    pub(super) payload_bytes: u64,
}

pub(super) enum CopyTransaction {
    Explicit,
    Implicit,
}

pub(super) async fn copy_table(
    source: &Session,
    destination: &Session,
    table: &PostgresTable,
    transaction: CopyTransaction,
) -> Result<TableWriteResult> {
    let relation = table.name.sql();
    let columns = table
        .columns
        .iter()
        .map(|column| escape_identifier(column))
        .collect::<Vec<_>>()
        .join(", ");
    let copy_filter = table
        .copy_filter
        .as_ref()
        .map(copy_condition)
        .transpose()?
        .map_or_else(String::new, |condition| where_clause(&condition));
    let copy_out =
        format!("COPY (SELECT {columns} FROM {relation}{copy_filter}) TO STDOUT (FORMAT TEXT)");
    let copy_columns = if columns.is_empty() {
        String::new()
    } else {
        format!(" ({columns})")
    };
    let copy_in = format!("COPY {relation}{copy_columns} FROM STDIN (FORMAT TEXT)");

    copy_rows(source, destination, &copy_out, &copy_in, transaction).await
}

pub(super) async fn delete_rows(destination: &Session, table: &PostgresTable) -> Result<()> {
    let mut sql = format!("DELETE FROM {}", table.name.sql());
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
    source: &Session,
    destination: &Session,
    copy_out: &str,
    copy_in: &str,
    transaction: CopyTransaction,
) -> Result<TableWriteResult> {
    let mut destination_guard = destination.client().connection().await?;
    let output = destination_guard
        .get()
        .await?
        .copy_in_raw(copy_in)
        .await
        .context("Start destination COPY")?;
    let mut source_guard = source.client().connection().await?;
    let input = match source_guard.get().await?.copy_out_raw(copy_out).await {
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
    tokio::pin!(input);
    let mut writer = DestinationCopyWriter {
        sink: output,
        payload_bytes: 0,
    };

    let copied: Result<()> = async {
        let mut buffer = BytesMut::with_capacity(COPY_BUFFER_BYTES);
        while let Some(mut chunk) = input
            .as_mut()
            .try_next()
            .await
            .context("Read source COPY")?
        {
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
    writer.finish(transaction).await
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

    async fn finish(self, transaction: CopyTransaction) -> Result<TableWriteResult> {
        let context = match transaction {
            CopyTransaction::Explicit => "Finish destination COPY",
            CopyTransaction::Implicit => "Commit destination COPY",
        };
        // Libpq completion consumes all results through ReadyForQuery, including
        // autocommit/deferred constraint errors, without an extra transaction.
        self.sink.finish().await.context(context)?;
        Ok(TableWriteResult {
            payload_bytes: self.payload_bytes,
        })
    }
}
