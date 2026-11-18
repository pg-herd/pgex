use crate::connection::Row;
use anyhow::{Context, Result};

use crate::{
    connection::{Connector, Session},
    table::QualifiedTable,
};

use super::layout::{self, Catalog};

pub struct CatalogReader {
    connector: Connector,
}

impl CatalogReader {
    pub fn new(connector: Connector) -> Self {
        Self { connector }
    }

    /// Lists ordinary tables and partition roots across non-system schemas.
    /// Partition children are omitted because selecting from a root includes them.
    pub async fn list_tables(&self) -> Result<Vec<QualifiedTable>> {
        let connection = self.connector.connect().await?;
        let rows = connection
            .client()
            .query(
                "SELECT n.nspname::text, c.relname::text
                 FROM pg_catalog.pg_class c
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
                 WHERE n.nspname !~ '^pg_' AND n.nspname <> 'information_schema'
                   AND c.relkind IN ('r', 'p') AND NOT c.relispartition
                 ORDER BY n.nspname, c.relname",
                &[],
            )
            .await;
        connection.close().await;

        rows.context("List tables in PostgreSQL catalog")?
            .iter()
            .map(|row| {
                Ok(QualifiedTable::from_catalog(
                    row.try_get(0)?,
                    row.try_get(1)?,
                ))
            })
            .collect()
    }

    /// Reads layouts on the supplied session while the selected table locks are held.
    pub async fn read_tables_on(
        &self,
        connection: &Session,
        tables: &[QualifiedTable],
    ) -> Result<Catalog> {
        layout::read_tables_on(connection, tables).await
    }
}
