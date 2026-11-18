use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};

use crate::{
    catalog::{Catalog, TableLayout},
    config::compiled::{ColumnRule, SourceFilterPolicy, TableFilter},
    predicate::Predicate,
    table::QualifiedTable,
};

pub struct SourcePlan<'a> {
    pub tables: Vec<SourceTable<'a>>,
}

pub struct SourceTable<'a> {
    pub layout: &'a TableLayout,
    pub filter: Option<&'a Predicate>,
}

pub fn plan_source_tables<'a>(
    catalog: &'a Catalog,
    filters: &'a SourceFilterPolicy,
) -> Result<SourcePlan<'a>> {
    let table_filters = index_table_filters(
        catalog.tables.iter().map(|layout| &layout.name),
        &filters.table_filters,
        "source.selection.table_filters",
    )?;
    let tables = catalog
        .tables
        .iter()
        .map(|layout| {
            let filter = table_filters
                .get(&layout.name)
                .copied()
                .or_else(|| first_matching_rule_filter(layout, &filters.table_rules))
                .or(filters.shared.as_ref());
            SourceTable { layout, filter }
        })
        .collect();
    Ok(SourcePlan { tables })
}

fn first_matching_rule_filter<'a>(
    layout: &TableLayout,
    rules: &'a [ColumnRule],
) -> Option<&'a Predicate> {
    rules
        .iter()
        .find(|rule| {
            layout
                .columns
                .iter()
                .any(|column| column.name == rule.has_column)
        })
        .map(|rule| &rule.condition)
}

pub(crate) fn index_table_filters<'table, 'filter>(
    selected_tables: impl IntoIterator<Item = &'table QualifiedTable>,
    filters: &'filter [TableFilter],
    field: &str,
) -> Result<BTreeMap<&'filter QualifiedTable, &'filter Predicate>> {
    let selected: BTreeSet<_> = selected_tables.into_iter().collect();
    let mut by_table = BTreeMap::new();
    for filter in filters {
        ensure!(
            selected.contains(&filter.table),
            "Table {} in {field} is outside the selected tables",
            filter.table
        );
        ensure!(
            by_table.insert(&filter.table, &filter.condition).is_none(),
            "Duplicate table {} in {field}",
            filter.table
        );
    }
    Ok(by_table)
}

#[cfg(test)]
mod tests;
