use std::path::Path;

use anyhow::{Context, Result, anyhow, ensure};

use crate::{
    predicate::{
        error_format::format_predicate_error,
        parameters::validation::{ParameterInput, ParameterOrigin},
        parser::parse_predicate,
    },
    table::QualifiedTable,
};

use super::schema;
use crate::config::input::{
    ColumnRuleInput, ConfigInput, ConnectionInput, FileConfig, PostgresSessionInput,
    PostgresTargetInput, PostgresTransferInput, PredicateInput, PredicateOrigin, SelectionInput,
    SnapshotInput, SourceInput, SourceSessionInput, TableFilterInput, TimeoutInput,
};

/// Reads a YAML config and parses every predicate before handing it to the compiler.
pub async fn load(path: &Path) -> Result<FileConfig> {
    let text = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("Read YAML config {}", path.display()))?;
    parse(&text, path)
}

pub(super) fn parse(text: &str, path: &Path) -> Result<FileConfig> {
    // Nearby YAML lines can contain connection credentials.
    let options = serde_saphyr::options! { with_snippet: false };
    let document: schema::Document = serde_saphyr::from_str_with_options(text, options)
        .map_err(|error| anyhow!("Invalid YAML in {}\n{error}", path.display()))?;
    let settings = file_settings(document, path)
        .map_err(|error| anyhow!("Invalid settings in {}\n{error:#}", path.display()))?;
    Ok(FileConfig { settings })
}

fn file_settings(document: schema::Document, path: &Path) -> Result<ConfigInput> {
    let source = document.source;
    let target = document.target;
    let params = document
        .params
        .into_iter()
        .map(|(name, value)| ParameterInput {
            name,
            raw_value: value.0,
            origin: ParameterOrigin::Yaml {
                file: path.to_owned(),
            },
        })
        .collect();

    Ok(ConfigInput {
        source: SourceInput {
            connection: connection(source.connection, "source.connection.url")?,
            session: SourceSessionInput {
                row_security_off: source.session.row_security_off,
            },
            snapshot: snapshot(source.snapshot)?,
            selection: selection(source.selection, path)?,
        },
        target: PostgresTargetInput {
            connection: connection(target.connection, "target.connection.url")?,
            session: PostgresSessionInput {
                row_security_off: target.session.row_security_off,
                replica_role: target.session.replica_role,
            },
            transfer: PostgresTransferInput {
                workers: target.transfer.workers,
                commit_group_mib: target.transfer.commit_group_mib,
                validate_layouts: target.transfer.validate_layouts,
                skip_delete: target.transfer.skip_delete,
            },
            delete_filters: target
                .delete_filters
                .map(|filters| {
                    filters
                        .into_iter()
                        .map(|(table, condition)| {
                            let field = format!("target.delete_filters.{table}");
                            table_filter(table, condition, path, field)
                        })
                        .collect::<Result<_>>()
                })
                .transpose()?,
        },
        params,
    })
}

fn connection(input: schema::Connection, field: &str) -> Result<ConnectionInput> {
    if let Some(url) = &input.url {
        ensure!(
            (url.starts_with("postgresql://") || url.starts_with("postgres://"))
                && url.parse::<tokio_postgres::Config>().is_ok(),
            "{field} must be a valid PostgreSQL URL"
        );
    }
    Ok(ConnectionInput {
        url: input.url,
        user: input.user,
        password: input.password,
        ca_file: input.ca_file,
    })
}

fn snapshot(input: schema::Snapshot) -> Result<SnapshotInput> {
    Ok(SnapshotInput {
        reader_connection_limit: input.reader_connection_limit,
        client_connection_check_interval_ms: milliseconds(
            input.client_connection_check_interval_ms,
            "source.snapshot.client_connection_check_interval_ms",
        )?,
        exporter_timeouts: timeouts(input.exporter_timeouts, "source.snapshot.exporter_timeouts")?,
        reader_timeouts: timeouts(input.reader_timeouts, "source.snapshot.reader_timeouts")?,
    })
}

fn timeouts(input: schema::Timeouts, field: &str) -> Result<TimeoutInput> {
    Ok(TimeoutInput {
        transaction_ms: milliseconds(input.transaction_ms, &format!("{field}.transaction_ms"))?,
        idle_in_transaction_ms: milliseconds(
            input.idle_in_transaction_ms,
            &format!("{field}.idle_in_transaction_ms"),
        )?,
    })
}

fn milliseconds(value: Option<u32>, field: &str) -> Result<Option<u32>> {
    ensure!(
        value.is_none_or(|value| i32::try_from(value).is_ok()),
        "{field} must be at most {}",
        i32::MAX
    );
    Ok(value)
}

fn selection(input: schema::Selection, path: &Path) -> Result<SelectionInput> {
    Ok(SelectionInput {
        tables: input
            .tables
            .map(|tables| {
                tables
                    .into_iter()
                    .enumerate()
                    .map(|(index, table)| {
                        table
                            .parse::<QualifiedTable>()
                            .with_context(|| format!("source.selection.tables[{index}]"))
                    })
                    .collect::<Result<_>>()
            })
            .transpose()?,
        shared: input
            .shared
            .map(|condition| predicate(condition, path, "source.selection.where".to_owned()))
            .transpose()?,
        table_rules: input
            .table_rules
            .map(|rules| {
                rules
                    .into_iter()
                    .enumerate()
                    .map(|(index, rule)| {
                        let field = format!("source.selection.table_rules[{index}].where");
                        Ok(ColumnRuleInput {
                            has_column: rule.has_column,
                            condition: predicate(rule.condition, path, field)?,
                        })
                    })
                    .collect::<Result<_>>()
            })
            .transpose()?,
        table_filters: input
            .table_filters
            .map(|filters| {
                filters
                    .into_iter()
                    .map(|(table, filter)| {
                        let field = format!("source.selection.table_filters.{table}.where");
                        table_filter(table, filter.condition, path, field)
                    })
                    .collect::<Result<_>>()
            })
            .transpose()?,
    })
}

fn table_filter(
    table: String,
    condition: schema::Text,
    path: &Path,
    field: String,
) -> Result<TableFilterInput> {
    let table = table.parse().with_context(|| field.clone())?;
    Ok(TableFilterInput {
        table,
        condition: predicate(condition, path, field)?,
    })
}

fn predicate(text: schema::Text, path: &Path, field: String) -> Result<PredicateInput> {
    let original = text.0;
    let parsed = parse_predicate(&original)
        .map_err(|error| anyhow!("{field}\n{}", format_predicate_error(&original, &error)))?;
    Ok(PredicateInput {
        original,
        parsed,
        origin: PredicateOrigin::Yaml {
            file: path.to_owned(),
            field,
        },
    })
}
