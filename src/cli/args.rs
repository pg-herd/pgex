use std::{
    env::VarError,
    num::{NonZeroU32, NonZeroUsize},
    path::PathBuf,
};

use anyhow::Result;
use clap::{CommandFactory, Parser, error::ErrorKind};

use pgex::{
    config::{
        input::{
            CliInput, ConfigInput, ConnectionInput, PostgresSessionInput, PostgresTargetInput,
            PostgresTransferInput, PredicateInput, PredicateOrigin, SelectionInput, SnapshotInput,
            SourceInput, SourceSessionInput, TimeoutInput,
        },
        snapshot_mode::SnapshotMode,
    },
    predicate::{
        error_format::format_predicate_error,
        parameters::validation::{ParameterInput, ParameterOrigin},
        parser::{is_parameter_name, parse_predicate},
    },
    table::QualifiedTable,
};

#[derive(Parser)]
#[command(
    about = "Refresh tables in a PostgreSQL destination",
    arg_required_else_help = true,
    after_help = r#"Examples:
  pgex --config pgex.yml
  pgex --source postgresql://sync_user@source.example.com/app \
       --target postgresql://sync_user@target.example.com/app

Supply passwords through PGEX_SOURCE_PASSWORD and PGEX_TARGET_PASSWORD."#
)]
pub(crate) struct Cli {
    /// Read settings from a YAML file.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Shared SQL WHERE condition for source tables.
    #[arg(long = "where", value_name = "PREDICATE")]
    where_clause: Option<String>,

    /// Supply a raw value for a named predicate parameter. May be repeated.
    #[arg(long, value_name = "NAME=VALUE")]
    param: Vec<String>,

    /// PostgreSQL URL or libpq-style connection string.
    #[arg(long)]
    source: Option<String>,

    /// Source username. Takes precedence over the YAML user and the URL username.
    #[arg(long)]
    source_user: Option<String>,

    /// Source password. Falls back to YAML, PGEX_SOURCE_PASSWORD, then the connection string.
    #[arg(long)]
    source_password: Option<String>,

    /// PEM certificate authority for the source. Defaults to the system certificate trust store.
    #[arg(long)]
    ca: Option<PathBuf>,

    /// Fail source transfers if a row-level security policy applies.
    #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    source_row_security_off: Option<bool>,

    /// Destination PostgreSQL URL or libpq-style connection string.
    #[arg(long)]
    target: Option<String>,

    /// Destination username. Takes precedence over the YAML user and the URL username.
    #[arg(long)]
    target_user: Option<String>,

    /// Destination password. Falls back to YAML, PGEX_TARGET_PASSWORD, then the connection string.
    #[arg(long)]
    target_password: Option<String>,

    /// PEM certificate authority for the destination. Defaults to the system certificate trust store.
    #[arg(long)]
    target_ca: Option<PathBuf>,

    /// Fail destination transfers if a row-level security policy applies.
    #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    target_row_security_off: Option<bool>,

    /// Compare source and destination table layouts before transfer.
    #[arg(long, action = clap::ArgAction::Set)]
    target_validate_layouts: Option<bool>,

    /// Skip the job-wide destination table locks. Layout validation still applies.
    #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    no_target_locks: Option<bool>,

    /// Maximum PostgreSQL transfer worker threads, capped by the source reader limit.
    #[arg(long)]
    target_parallel_transfers: Option<NonZeroUsize>,

    /// Commit after a whole table brings the destination's copied payload to this size in MiB.
    #[arg(long)]
    target_commit_group_mib: Option<NonZeroU32>,

    /// Use PostgreSQL replica trigger behavior inside destination commit groups.
    #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    target_replica_role: Option<bool>,

    /// Copy selected rows without deleting existing destination rows.
    #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    target_skip_delete: Option<bool>,

    /// TRUNCATE each destination table with CASCADE before copying. The truncate commits separately.
    #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    target_truncate: Option<bool>,

    /// Comma-separated source tables, each as schema.table. Omit to discover tables in all non-system schemas.
    #[arg(long, value_name = "TABLE_LIST", value_delimiter = ',', action = clap::ArgAction::Set)]
    tables: Option<Vec<QualifiedTable>>,

    /// Source snapshot mode. Defaults to shared when omitted from CLI and YAML.
    #[arg(long, value_enum, value_name = "MODE")]
    source_snapshot: Option<SnapshotMode>,

    /// Maximum concurrent source reader sessions. The source table locks use a separate session.
    #[arg(long)]
    source_reader_limit: Option<NonZeroUsize>,

    /// Maximum source table lock transaction lifetime in milliseconds (PostgreSQL 17+).
    #[arg(long, value_parser = clap::value_parser!(u32).range(..=i64::from(i32::MAX)))]
    source_exporter_transaction_timeout_ms: Option<u32>,

    /// Maximum idle time in the source table lock transaction in milliseconds.
    #[arg(long, value_parser = clap::value_parser!(u32).range(..=i64::from(i32::MAX)))]
    source_exporter_idle_in_transaction_timeout_ms: Option<u32>,

    /// Maximum source reader transaction lifetime in milliseconds (PostgreSQL 17+).
    #[arg(long, value_parser = clap::value_parser!(u32).range(..=i64::from(i32::MAX)))]
    source_reader_transaction_timeout_ms: Option<u32>,

    /// Maximum idle time in a source reader transaction in milliseconds.
    #[arg(long, value_parser = clap::value_parser!(u32).range(..=i64::from(i32::MAX)))]
    source_reader_idle_in_transaction_timeout_ms: Option<u32>,

    /// Milliseconds between PostgreSQL disconnect checks during source transactions. Zero disables checks.
    #[arg(long, value_parser = clap::value_parser!(u32).range(..=i64::from(i32::MAX)))]
    client_connection_check_interval_ms: Option<u32>,
}

pub(crate) struct ParsedCli {
    pub config_path: Option<PathBuf>,
    pub input: CliInput,
}

impl Cli {
    pub(crate) fn into_input(self) -> Result<ParsedCli, clap::Error> {
        let shared = self.where_clause.map(parse_where).transpose()?;
        let params = self
            .param
            .iter()
            .map(|value| parse_parameter(value))
            .collect::<Result<_, _>>()?;

        Ok(ParsedCli {
            config_path: self.config,
            input: CliInput {
                settings: ConfigInput {
                    source: SourceInput {
                        connection: ConnectionInput {
                            url: self.source,
                            user: self.source_user,
                            password: self.source_password,
                            ca_file: self.ca,
                        },
                        session: SourceSessionInput {
                            row_security_off: self.source_row_security_off,
                        },
                        snapshot: SnapshotInput {
                            mode: self.source_snapshot,
                            reader_connection_limit: self.source_reader_limit,
                            client_connection_check_interval_ms: self
                                .client_connection_check_interval_ms,
                            exporter_timeouts: TimeoutInput {
                                transaction_ms: self.source_exporter_transaction_timeout_ms,
                                idle_in_transaction_ms: self
                                    .source_exporter_idle_in_transaction_timeout_ms,
                            },
                            reader_timeouts: TimeoutInput {
                                transaction_ms: self.source_reader_transaction_timeout_ms,
                                idle_in_transaction_ms: self
                                    .source_reader_idle_in_transaction_timeout_ms,
                            },
                        },
                        selection: SelectionInput {
                            tables: self.tables,
                            shared,
                            ..SelectionInput::default()
                        },
                    },
                    target: PostgresTargetInput {
                        connection: ConnectionInput {
                            url: self.target,
                            user: self.target_user,
                            password: self.target_password,
                            ca_file: self.target_ca,
                        },
                        session: PostgresSessionInput {
                            row_security_off: self.target_row_security_off,
                            replica_role: self.target_replica_role,
                        },
                        transfer: PostgresTransferInput {
                            workers: self.target_parallel_transfers,
                            commit_group_mib: self.target_commit_group_mib,
                            validate_layouts: self.target_validate_layouts,
                            no_locks: self.no_target_locks,
                            skip_delete: self.target_skip_delete,
                            truncate: self.target_truncate,
                        },
                        ..PostgresTargetInput::default()
                    },
                    params,
                },
            },
        })
    }
}

fn parse_where(original: String) -> Result<PredicateInput, clap::Error> {
    let parsed = parse_predicate(&original).map_err(|error| {
        let detail = format_predicate_error(&original, &error);
        argument_error(format!("Invalid --where predicate\n{detail}"))
    })?;
    Ok(PredicateInput {
        original,
        parsed,
        origin: PredicateOrigin::Cli {
            flag: "--where".to_owned(),
        },
    })
}

fn parse_parameter(argument: &str) -> Result<ParameterInput, clap::Error> {
    let Some((name, value)) = argument.split_once('=') else {
        return Err(argument_error("Expected --param NAME=VALUE"));
    };
    if !is_parameter_name(name) {
        return Err(argument_error(
            "Expected an ASCII parameter name in --param NAME=VALUE",
        ));
    }
    Ok(ParameterInput {
        name: name.to_owned(),
        raw_value: value.to_owned(),
        origin: ParameterOrigin::Cli,
    })
}

fn argument_error(message: impl std::fmt::Display) -> clap::Error {
    clap::Error::raw(ErrorKind::ValueValidation, message).format(&mut Cli::command())
}

pub(crate) fn resolve_password(
    explicit: Option<String>,
    environment_variable: &str,
    read_environment: impl FnOnce() -> std::result::Result<String, VarError>,
) -> Result<Option<String>> {
    if let Some(password) = explicit {
        return Ok(Some(password));
    }

    match read_environment() {
        Ok(password) => Ok(Some(password)),
        Err(VarError::NotPresent) => Ok(None),
        Err(VarError::NotUnicode(_)) => anyhow::bail!("{environment_variable} must be valid UTF-8"),
    }
}

#[cfg(test)]
mod tests;
