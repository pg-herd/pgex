mod estimate;
mod statistics;

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;

use crate::{
    connection::{Connector, Session},
    table::QualifiedTable,
};

use self::{estimate::estimate_table, statistics::read_statistics};

#[derive(Clone, Copy, Debug)]
pub enum TenantScope<'a> {
    All,
    /// The selected values of a PostgreSQL smallint, integer or bigint column.
    Selected {
        column: &'a str,
        ids: &'a [i64],
    },
}

pub type SizeEstimates = BTreeMap<QualifiedTable, Option<TableSizeEstimate>>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableSizeEstimate {
    pub estimated_rows: f64,
    pub estimated_bytes: u64,
}

pub struct TableSizeEstimator {
    connector: Connector,
}

impl TableSizeEstimator {
    pub fn new(connector: Connector) -> Self {
        Self { connector }
    }

    /// Estimates rows and bytes from catalog statistics on a short-lived connection.
    /// The byte estimate includes binary COPY framing. It uses TOAST heap size as a
    /// proxy for out-of-line values and prorates it by the estimated share of rows.
    /// An entry with no estimate means the statistics are unavailable or unsupported.
    pub async fn estimate_selected_sizes(
        &self,
        tables: &[QualifiedTable],
        tenants: TenantScope<'_>,
    ) -> Result<SizeEstimates> {
        if tables.is_empty() {
            return Ok(SizeEstimates::new());
        }

        self.connector
            .with_connection(async |session| {
                Self::estimate_selected_sizes_on(session, tables, tenants).await
            })
            .await
    }

    /// Estimates sizes using the supplied source session.
    pub async fn estimate_selected_sizes_on(
        session: &Session,
        tables: &[QualifiedTable],
        tenants: TenantScope<'_>,
    ) -> Result<SizeEstimates> {
        if tables.is_empty() {
            return Ok(SizeEstimates::new());
        }

        let (column, selected_ids) = match tenants {
            TenantScope::All => (None, None),
            TenantScope::Selected { column, ids } => (
                Some(column),
                Some(ids.iter().copied().collect::<BTreeSet<_>>()),
            ),
        };

        let statistics = read_statistics(session, tables, column).await?;

        let mut estimates: SizeEstimates =
            tables.iter().cloned().map(|name| (name, None)).collect();
        for stats in statistics {
            let estimate = estimate_table(&stats, selected_ids.as_ref());
            estimates.insert(stats.table, estimate);
        }
        Ok(estimates)
    }
}
