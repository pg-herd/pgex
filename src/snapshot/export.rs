use crate::connection::Row;
use crate::connection::Session;
use anyhow::{Context, Result};

pub(super) async fn export_snapshot(session: &Session) -> Result<String> {
    let row = session
        .client()
        .query_one("SELECT pg_catalog.pg_export_snapshot()", &[])
        .await
        .context("Export source snapshot")?;
    row.try_get(0).context("Read exported snapshot key")
}
