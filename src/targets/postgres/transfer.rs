use std::pin::Pin;

use anyhow::{Context, Result};
use bytes::{Buf, Bytes, BytesMut};
use futures_util::{SinkExt, TryStreamExt};
use postgres_protocol::escape::escape_identifier;
use tokio_postgres::CopyInSink;

use crate::{connection::Session, predicate::Predicate};

use super::{
    PostgresTable,
    predicate::{bind_value, copy_condition, where_clause},
};

const COPY_BUFFER_BYTES: usize = 256 * 1024;

pub(super) struct TableWriteResult {
    /// Encoded payload accepted by the destination.
    pub(super) payload_bytes: u64,
}

pub(super) async fn transfer_table(
    source: &Session,
    destination: &Session,
    table: &PostgresTable<'_>,
    skip_delete: bool,
) -> Result<TableWriteResult> {
    let relation = table.layout.name.sql();
    let columns = table
        .layout
        .columns
        .iter()
        .filter(|column| !column.generated)
        .map(|column| escape_identifier(&column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let copy_filter = table
        .copy_filter
        .map(copy_condition)
        .transpose()?
        .map_or_else(String::new, |condition| where_clause(&condition));
    let copy_out =
        format!("COPY (SELECT {columns} FROM {relation}{copy_filter}) TO STDOUT (FORMAT BINARY)");
    let copy_columns = if columns.is_empty() {
        String::new()
    } else {
        format!(" ({columns})")
    };
    let copy_in = format!("COPY {relation}{copy_columns} FROM STDIN (FORMAT BINARY)");

    if !skip_delete {
        delete_rows(destination, &relation, table.delete_filter).await?;
    }
    copy_binary(source, destination, &copy_out, &copy_in).await
}

async fn delete_rows(
    destination: &Session,
    relation: &str,
    predicate: Option<&Predicate>,
) -> Result<()> {
    let mut sql = format!("DELETE FROM {relation}");
    let mut binds = Vec::new();
    if let Some(predicate) = predicate {
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
        .execute_typed(&sql, &binds)
        .await
        .context("Delete destination rows")?;
    Ok(())
}

async fn copy_binary(
    source: &Session,
    destination: &Session,
    copy_out: &str,
    copy_in: &str,
) -> Result<TableWriteResult> {
    let input = source
        .client()
        .copy_out(copy_out)
        .await
        .context("Start source COPY")?;
    let output = destination
        .client()
        .copy_in::<_, Bytes>(copy_in)
        .await
        .context("Start destination COPY")?;
    tokio::pin!(input, output);
    let mut writer = DestinationCopyWriter {
        sink: output.as_mut(),
        payload_bytes: 0,
    };

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
    writer.finish().await
}

struct DestinationCopyWriter<'a> {
    sink: Pin<&'a mut CopyInSink<Bytes>>,
    payload_bytes: u64,
}

impl DestinationCopyWriter<'_> {
    async fn write(&mut self, payload: Bytes) -> Result<()> {
        let length = payload.len() as u64;
        self.sink
            .as_mut()
            .feed(payload)
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
