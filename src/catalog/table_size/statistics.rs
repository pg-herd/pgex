use anyhow::{Context, Result};
use tokio_postgres::Row;

use crate::{connection::Session, table::QualifiedTable};

use super::estimate::{TableStatistics, TenantStatistics};

pub(super) async fn read_statistics(
    connection: &Session,
    tables: &[QualifiedTable],
    tenant_column: Option<&str>,
) -> Result<Vec<TableStatistics>> {
    let (schemas, names): (Vec<_>, Vec<_>) = tables
        .iter()
        .map(|table| (table.schema(), table.table()))
        .unzip();
    let rows = connection
        .client()
        .query(
            include_str!("estimates.sql"),
            &[&schemas, &names, &tenant_column],
        )
        .await
        .context("Read PostgreSQL table size statistics")?;

    rows.into_iter()
        .map(TableStatistics::try_from)
        .collect::<Result<Vec<_>>>()
        .context("Decode PostgreSQL table size statistics")
}

impl TryFrom<Row> for TableStatistics {
    type Error = anyhow::Error;

    fn try_from(row: Row) -> Result<Self> {
        Ok(Self {
            table: QualifiedTable::from_catalog(
                row.try_get("schema_name")?,
                row.try_get("table_name")?,
            ),
            total_rows: row.try_get("total_rows")?,
            bytes_per_row: row.try_get("bytes_per_row")?,
            toast_bytes: row.try_get("toast_bytes")?,
            tenants: TenantStatistics {
                null_fraction: row.try_get("tenant_null_fraction")?,
                distinct: row.try_get("tenant_distinct")?,
                common_ids: row.try_get("common_ids")?,
                common_frequencies: row.try_get("common_frequencies")?,
            },
        })
    }
}
