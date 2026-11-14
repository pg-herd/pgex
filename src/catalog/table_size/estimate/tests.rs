use std::collections::BTreeSet;

use super::{TableSizeEstimate, TableStatistics, TenantStatistics, copy_size, estimate_table};

#[test]
fn estimates_common_and_unlisted_tenants_from_separate_frequencies() {
    let stats = TenantStatistics {
        null_fraction: Some(0.125),
        distinct: Some(4.0),
        common_ids: Some(vec![10, 20]),
        common_frequencies: Some(vec![0.5, 0.125]),
    };
    let selected = BTreeSet::from([10, 30]);

    assert_eq!(stats.selected_fraction(100.0, &selected), Some(0.625));
}

#[test]
fn interprets_negative_distinct_as_a_fraction_of_table_rows() {
    let stats = TenantStatistics {
        null_fraction: Some(0.0),
        distinct: Some(-0.5),
        common_ids: Some(vec![]),
        common_frequencies: Some(vec![]),
    };

    assert_eq!(
        stats.selected_fraction(16.0, &BTreeSet::from([10, 20])),
        Some(0.25)
    );
}

#[test]
fn caps_unlisted_tenants_at_the_remaining_non_null_fraction() {
    let stats = TenantStatistics {
        null_fraction: Some(0.25),
        distinct: Some(2.0),
        common_ids: Some(vec![10]),
        common_frequencies: Some(vec![0.5]),
    };

    assert_eq!(
        stats.selected_fraction(100.0, &BTreeSet::from([10, 20, 30])),
        Some(0.75)
    );
}

#[test]
fn reports_unknown_fraction_when_tenant_statistics_are_missing() {
    let stats = TenantStatistics {
        null_fraction: None,
        distinct: None,
        common_ids: None,
        common_frequencies: Some(vec![]),
    };

    assert_eq!(stats.selected_fraction(100.0, &BTreeSet::from([10])), None);
}

#[test]
fn adds_stream_framing_and_toast_and_rounds_up() {
    let estimate = copy_size(2.5, 11.0, 100.0);

    assert_eq!(estimate.estimated_rows, 2.5);
    assert_eq!(estimate.estimated_bytes, 149);
    assert_eq!(copy_size(0.0, 0.0, 0.0).estimated_bytes, 21);
}

#[test]
fn empty_selection_has_only_stream_framing_even_without_statistics() {
    let stats = TableStatistics {
        table: "public.events".parse().unwrap(),
        total_rows: -1.0,
        bytes_per_row: None,
        toast_bytes: None,
        tenants: TenantStatistics {
            null_fraction: None,
            distinct: None,
            common_ids: None,
            common_frequencies: None,
        },
    };

    assert_eq!(
        estimate_table(&stats, Some(&BTreeSet::new())),
        Some(TableSizeEstimate {
            estimated_rows: 0.0,
            estimated_bytes: 21,
        })
    );
}
