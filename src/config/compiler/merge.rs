use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, bail};

use crate::{
    predicate::parameters::validation::{ParameterInput, ParameterOrigin},
    snapshot::{
        DEFAULT_CLIENT_CONNECTION_CHECK_INTERVAL_MS, DEFAULT_READER_CONNECTION_LIMIT,
        SnapshotTimeouts,
    },
    targets::{DEFAULT_COMMIT_GROUP_MIB, DEFAULT_PARALLEL_TRANSFERS},
};

use super::super::{
    compiled::{
        ConnectionConfig, PostgresSessionSettings, PostgresTransferSettings, SnapshotSettings,
        SourceSessionSettings,
    },
    input::{
        ConnectionInput, PostgresSessionInput, PostgresTransferInput, SelectionInput,
        SnapshotInput, SourceSessionInput, TimeoutInput,
    },
};

pub(super) struct Parameters<'a> {
    pub(super) values: HashMap<&'a str, &'a ParameterInput>,
    pub(super) used: HashSet<&'a str>,
}

pub(super) fn parameters<'a>(
    cli: &'a [ParameterInput],
    file: &'a [ParameterInput],
) -> Result<Parameters<'a>> {
    let mut values = parameters_by_name(file)?;
    values.extend(parameters_by_name(cli)?);
    Ok(Parameters {
        values,
        used: HashSet::new(),
    })
}

fn parameters_by_name(inputs: &[ParameterInput]) -> Result<HashMap<&str, &ParameterInput>> {
    let mut values = HashMap::with_capacity(inputs.len());
    for input in inputs {
        if values.insert(input.name.as_str(), input).is_some() {
            bail!(
                "Duplicate parameter '{}' from {}",
                input.name,
                value_origin(&input.origin)
            );
        }
    }
    Ok(values)
}

pub(super) fn value_origin(origin: &ParameterOrigin) -> String {
    match origin {
        ParameterOrigin::Cli => "--param".to_owned(),
        ParameterOrigin::Yaml { file } => format!("{} (params)", file.display()),
    }
}

pub(super) fn selection(cli: SelectionInput, file: SelectionInput) -> SelectionInput {
    SelectionInput {
        tables: cli.tables.or(file.tables),
        shared: cli.shared.or(file.shared),
        table_rules: cli.table_rules.or(file.table_rules),
        table_filters: cli.table_filters.or(file.table_filters),
    }
}

pub(super) fn connection(
    cli: ConnectionInput,
    file: ConnectionInput,
    field: &str,
) -> Result<ConnectionConfig> {
    Ok(ConnectionConfig {
        url: cli
            .url
            .or(file.url)
            .with_context(|| format!("Missing {field}.url"))?,
        user: cli.user.or(file.user),
        password: cli.password.or(file.password),
        ca_file: cli.ca_file.or(file.ca_file),
    })
}

pub(super) fn source_session(
    cli: SourceSessionInput,
    file: SourceSessionInput,
) -> SourceSessionSettings {
    SourceSessionSettings {
        row_security_off: cli
            .row_security_off
            .or(file.row_security_off)
            .unwrap_or_default(),
    }
}

pub(super) fn snapshot(cli: SnapshotInput, file: SnapshotInput) -> SnapshotSettings {
    SnapshotSettings {
        mode: cli.mode.or(file.mode).unwrap_or_default(),
        reader_connection_limit: cli
            .reader_connection_limit
            .or(file.reader_connection_limit)
            .unwrap_or(DEFAULT_READER_CONNECTION_LIMIT),
        client_connection_check_interval_ms: cli
            .client_connection_check_interval_ms
            .or(file.client_connection_check_interval_ms)
            .unwrap_or(DEFAULT_CLIENT_CONNECTION_CHECK_INTERVAL_MS),
        exporter_timeouts: timeouts(cli.exporter_timeouts, file.exporter_timeouts),
        reader_timeouts: timeouts(cli.reader_timeouts, file.reader_timeouts),
    }
}

fn timeouts(cli: TimeoutInput, file: TimeoutInput) -> SnapshotTimeouts {
    SnapshotTimeouts {
        transaction_ms: cli
            .transaction_ms
            .or(file.transaction_ms)
            .unwrap_or_default(),
        idle_in_transaction_ms: cli
            .idle_in_transaction_ms
            .or(file.idle_in_transaction_ms)
            .unwrap_or_default(),
    }
}

pub(super) fn postgres_session(
    cli: PostgresSessionInput,
    file: PostgresSessionInput,
) -> PostgresSessionSettings {
    PostgresSessionSettings {
        row_security_off: cli
            .row_security_off
            .or(file.row_security_off)
            .unwrap_or_default(),
        replica_role: cli.replica_role.or(file.replica_role).unwrap_or_default(),
    }
}

pub(super) fn postgres_transfer(
    cli: PostgresTransferInput,
    file: PostgresTransferInput,
) -> PostgresTransferSettings {
    PostgresTransferSettings {
        workers: cli
            .workers
            .or(file.workers)
            .unwrap_or(DEFAULT_PARALLEL_TRANSFERS),
        commit_group_mib: cli
            .commit_group_mib
            .or(file.commit_group_mib)
            .unwrap_or(DEFAULT_COMMIT_GROUP_MIB),
        validate_layouts: cli
            .validate_layouts
            .or(file.validate_layouts)
            .unwrap_or(true),
        no_locks: cli.no_locks.or(file.no_locks).unwrap_or_default(),
        skip_delete: cli.skip_delete.or(file.skip_delete).unwrap_or_default(),
        truncate: cli.truncate.or(file.truncate).unwrap_or_default(),
    }
}
