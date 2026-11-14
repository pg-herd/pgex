use std::collections::BTreeSet;

use anyhow::{Result, ensure};

use super::super::{
    compiled::{
        ColumnRule, CompiledConfig, PostgresTargetConfig, SelectionOptions, SourceConfig,
        SourceFilterPolicy, TableFilter, TableSelection,
    },
    input::{CliInput, FileConfig, SelectionInput, TableFilterInput},
};
use super::{
    merge::{self, Parameters},
    predicates::{compile_predicate, reject_unused_parameters},
};
use crate::table::QualifiedTable;

pub fn compile_config(cli: CliInput, file: Option<FileConfig>) -> Result<CompiledConfig> {
    let cli = cli.settings;
    let file = file.map(|file| file.settings).unwrap_or_default();

    let source_connection = merge::connection(
        cli.source.connection,
        file.source.connection,
        "source.connection",
    )?;
    let target_connection = merge::connection(
        cli.target.connection,
        file.target.connection,
        "target.connection",
    )?;

    let mut selection = merge::selection(cli.source.selection, file.source.selection);
    let tables = compile_table_selection(selection.tables.take())?;
    let delete_filters = cli
        .target
        .delete_filters
        .or(file.target.delete_filters)
        .unwrap_or_default();
    let mut parameters = merge::parameters(&cli.params, &file.params)?;
    let filters = compile_source_filters(selection, &tables, &mut parameters)?;
    let delete_filters = compile_table_filters(
        delete_filters,
        &tables,
        "target.delete_filters",
        &mut parameters,
    )?;
    reject_unused_parameters(&parameters)?;

    Ok(CompiledConfig {
        source: SourceConfig {
            connection: source_connection,
            session: merge::source_session(cli.source.session, file.source.session),
            snapshot: merge::snapshot(cli.source.snapshot, file.source.snapshot),
            selection: SelectionOptions { tables, filters },
        },
        target: PostgresTargetConfig {
            connection: target_connection,
            session: merge::postgres_session(cli.target.session, file.target.session),
            transfer: merge::postgres_transfer(cli.target.transfer, file.target.transfer),
            delete_filters,
        },
    })
}

fn compile_table_selection(tables: Option<Vec<QualifiedTable>>) -> Result<TableSelection> {
    let Some(tables) = tables else {
        return Ok(TableSelection::Discover);
    };
    ensure!(
        !tables.is_empty(),
        "source.selection.tables must contain at least one table"
    );
    let mut seen = BTreeSet::new();
    for table in &tables {
        ensure!(
            seen.insert(table),
            "Duplicate table {} in source.selection.tables",
            table.sql()
        );
    }
    Ok(TableSelection::Explicit(tables))
}

fn compile_source_filters(
    selection: SelectionInput,
    tables: &TableSelection,
    params: &mut Parameters<'_>,
) -> Result<SourceFilterPolicy> {
    let rules = selection.table_rules.unwrap_or_default();
    let filters = selection.table_filters.unwrap_or_default();

    let shared = selection
        .shared
        .map(|input| compile_predicate(input, params))
        .transpose()?;
    let table_rules = rules
        .into_iter()
        .map(|rule| {
            Ok(ColumnRule {
                has_column: rule.has_column,
                condition: compile_predicate(rule.condition, params)?,
            })
        })
        .collect::<Result<_>>()?;
    let table_filters =
        compile_table_filters(filters, tables, "source.selection.table_filters", params)?;
    Ok(SourceFilterPolicy {
        shared,
        table_rules,
        table_filters,
    })
}

fn compile_table_filters(
    filters: Vec<TableFilterInput>,
    tables: &TableSelection,
    field: &str,
    params: &mut Parameters<'_>,
) -> Result<Vec<TableFilter>> {
    let mut seen = BTreeSet::new();
    let mut compiled = Vec::with_capacity(filters.len());
    for filter in filters {
        ensure!(
            seen.insert(filter.table.clone()),
            "Duplicate table {} in {field}",
            filter.table.sql()
        );
        if let TableSelection::Explicit(tables) = tables {
            ensure!(
                tables.contains(&filter.table),
                "Table {} in {field} is outside source.selection.tables",
                filter.table.sql()
            );
        }
        compiled.push(TableFilter {
            table: filter.table,
            condition: compile_predicate(filter.condition, params)?,
        });
    }
    Ok(compiled)
}
