use std::collections::BTreeSet;

use crate::table::QualifiedTable;

use super::TableSizeEstimate;

const COPY_STREAM_OVERHEAD: f64 = 21.0;

pub(super) struct TableStatistics {
    pub(super) table: QualifiedTable,
    pub(super) total_rows: f64,
    pub(super) bytes_per_row: Option<f64>,
    pub(super) toast_bytes: Option<i64>,
    pub(super) tenants: TenantStatistics,
}

pub(super) struct TenantStatistics {
    pub(super) null_fraction: Option<f32>,
    pub(super) distinct: Option<f32>,
    pub(super) common_ids: Option<Vec<i64>>,
    pub(super) common_frequencies: Option<Vec<f32>>,
}

pub(super) fn estimate_table(
    stats: &TableStatistics,
    selected_ids: Option<&BTreeSet<i64>>,
) -> Option<TableSizeEstimate> {
    let total_rows = stats.total_rows;
    if total_rows == 0.0 || selected_ids.is_some_and(BTreeSet::is_empty) {
        return Some(copy_size(0.0, 0.0, 0.0));
    }
    if total_rows < 0.0 {
        return None;
    }

    let fraction = match selected_ids {
        Some(ids) => stats.tenants.selected_fraction(total_rows, ids)?,
        None => 1.0,
    };
    if fraction == 0.0 {
        return Some(copy_size(0.0, 0.0, 0.0));
    }

    Some(copy_size(
        total_rows * fraction,
        stats.bytes_per_row?,
        stats.toast_bytes? as f64 * fraction,
    ))
}

impl TenantStatistics {
    fn selected_fraction(&self, total_rows: f64, selected_ids: &BTreeSet<i64>) -> Option<f64> {
        let null_fraction = f64::from(self.null_fraction?);
        let distinct = f64::from(self.distinct?);
        let common_ids = self.common_ids.as_deref()?;
        let common_frequencies = self.common_frequencies.as_deref()?;
        if common_ids.len() != common_frequencies.len() {
            return None;
        }

        let distinct = if distinct < 0.0 {
            -distinct * total_rows
        } else {
            distinct
        };
        let mut selected_common_count = 0;
        let mut selected_common_fraction = 0.0;
        let mut common_fraction = 0.0;
        for (id, frequency) in common_ids.iter().zip(common_frequencies) {
            let frequency = f64::from(*frequency);
            common_fraction += frequency;
            if selected_ids.contains(id) {
                selected_common_count += 1;
                selected_common_fraction += frequency;
            }
        }

        let remaining_fraction = (1.0 - null_fraction - common_fraction).max(0.0);
        let remaining_distinct = (distinct - common_ids.len() as f64).max(0.0);
        let selected_distinct = (selected_ids.len() - selected_common_count) as f64;
        let selected_remaining_fraction = if remaining_distinct > 0.0 {
            remaining_fraction * (selected_distinct / remaining_distinct).min(1.0)
        } else {
            0.0
        };
        Some((selected_common_fraction + selected_remaining_fraction).clamp(0.0, 1.0))
    }
}

fn copy_size(estimated_rows: f64, bytes_per_row: f64, toast_bytes: f64) -> TableSizeEstimate {
    TableSizeEstimate {
        estimated_rows,
        estimated_bytes: (COPY_STREAM_OVERHEAD + estimated_rows * bytes_per_row + toast_bytes)
            .ceil() as u64,
    }
}

#[cfg(test)]
mod tests;
