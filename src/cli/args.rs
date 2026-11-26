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
    about = "Export PostgreSQL data",
    arg_required_else_help = true,
    after_help = r#"Examples:
  pgex --config pgex.yml
  pgex --source postgresql://sync_user@source.example.com/app \
       --target postgresql://sync_user@target.example.com/app

Supply passwords through PGEX_SOURCE_PASSWORD and PGEX_TARGET_PASSWORD.
Boolean flags accept =true or =false.

Run pgex --help to see all options."#
)]
pub(crate) struct Cli {
    /// Read settings from a YAML file.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Shared SQL WHERE condition for source tables.
    #[arg(long = "where", value_name = "PREDICATE", help_heading = "Selection")]
    where_clause: Option<String>,

    /// Set a named parameter; repeat as needed.
    #[arg(long, value_name = "NAME=VALUE", help_heading = "Selection")]
    param: Vec<String>,

    /// PostgreSQL connection URL.
    #[arg(long, value_name = "URL", help_heading = "Source")]
    source: Option<String>,

    /// Source username (CLI > YAML > connection string).
    #[arg(
        hide_short_help = true,
        long,
        value_name = "USER",
        help_heading = "Source"
    )]
    source_user: Option<String>,

    /// Password (CLI > YAML > PGEX_SOURCE_PASSWORD > connection string).
    #[arg(
        hide_short_help = true,
        long,
        value_name = "PASSWORD",
        help_heading = "Source"
    )]
    source_password: Option<String>,

    /// Source CA certificate in PEM format. Defaults to system trust.
    #[arg(
        hide_short_help = true,
        long,
        value_name = "PATH",
        help_heading = "Source"
    )]
    ca: Option<PathBuf>,

    /// Fail source transfers if a row-level security policy applies.
    #[arg(hide_short_help = true, long, value_name = "BOOL", hide_possible_values = true, help_heading = "Source", num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    source_row_security_off: Option<bool>,

    /// PostgreSQL connection URL.
    #[arg(long, value_name = "URL", help_heading = "Destination")]
    target: Option<String>,

    /// Destination username (CLI > YAML > connection string).
    #[arg(
        hide_short_help = true,
        long,
        value_name = "USER",
        help_heading = "Destination"
    )]
    target_user: Option<String>,

    /// Password (CLI > YAML > PGEX_TARGET_PASSWORD > connection string).
    #[arg(
        hide_short_help = true,
        long,
        value_name = "PASSWORD",
        help_heading = "Destination"
    )]
    target_password: Option<String>,

    /// Destination CA certificate in PEM format. Defaults to system trust.
    #[arg(
        hide_short_help = true,
        long,
        value_name = "PATH",
        help_heading = "Destination"
    )]
    target_ca: Option<PathBuf>,

    /// Fail destination transfers if a row-level security policy applies.
    #[arg(hide_short_help = true, long, value_name = "BOOL", hide_possible_values = true, help_heading = "Destination", num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    target_row_security_off: Option<bool>,

    /// Compare source and destination table layouts before transfer.
    #[arg(hide_short_help = true, long, value_name = "BOOL", hide_possible_values = true, help_heading = "Transfers", action = clap::ArgAction::Set)]
    target_validate_layouts: Option<bool>,

    /// Skip job-wide destination locks. Layout validation still applies.
    #[arg(hide_short_help = true, long, value_name = "BOOL", hide_possible_values = true, help_heading = "Transfers", num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    no_target_locks: Option<bool>,

    /// Transfer worker threads, capped by the source reader limit.
    #[arg(
        hide_short_help = true,
        long,
        value_name = "COUNT",
        help_heading = "Transfers"
    )]
    target_parallel_transfers: Option<NonZeroUsize>,

    /// Commit once a whole table brings copied bytes to this size in MiB.
    #[arg(
        hide_short_help = true,
        long,
        value_name = "MIB",
        help_heading = "Transfers"
    )]
    target_commit_group_mib: Option<NonZeroU32>,

    /// Use replica trigger behavior inside destination commit groups.
    #[arg(hide_short_help = true, long, value_name = "BOOL", hide_possible_values = true, help_heading = "Transfers", num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    target_replica_role: Option<bool>,

    /// Copy selected rows without deleting existing destination rows.
    #[arg(hide_short_help = true, long, value_name = "BOOL", hide_possible_values = true, help_heading = "Transfers", num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    target_skip_delete: Option<bool>,

    /// TRUNCATE tables with CASCADE and COPY in one transaction per table.
    #[arg(hide_short_help = true, long, value_name = "BOOL", hide_possible_values = true, help_heading = "Transfers", num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    target_truncate: Option<bool>,

    /// Copy these schema.table names (comma-separated).
    #[arg(long, value_name = "TABLES", help_heading = "Selection", value_delimiter = ',', action = clap::ArgAction::Set)]
    tables: Option<Vec<QualifiedTable>>,

    /// Use shared (default) or per-statement snapshots.
    #[arg(
        long,
        value_enum,
        value_name = "MODE",
        hide_possible_values = true,
        help_heading = "Snapshot",
        hide_short_help = true
    )]
    source_snapshot: Option<SnapshotMode>,

    /// Maximum source readers. Source locks use a separate session.
    #[arg(
        hide_short_help = true,
        long,
        value_name = "COUNT",
        help_heading = "Snapshot"
    )]
    source_reader_limit: Option<NonZeroUsize>,

    /// Source lock transaction timeout in milliseconds (PostgreSQL 17+).
    #[arg(hide_short_help = true, long, value_name = "MS", help_heading = "Timeouts", value_parser = clap::value_parser!(u32).range(..=i64::from(i32::MAX)))]
    source_exporter_transaction_timeout_ms: Option<u32>,

    /// Maximum idle time in the source table lock transaction in milliseconds.
    #[arg(hide_short_help = true, long, value_name = "MS", help_heading = "Timeouts", value_parser = clap::value_parser!(u32).range(..=i64::from(i32::MAX)))]
    source_exporter_idle_in_transaction_timeout_ms: Option<u32>,

    /// Source reader transaction timeout in milliseconds (PostgreSQL 17+).
    #[arg(hide_short_help = true, long, value_name = "MS", help_heading = "Timeouts", value_parser = clap::value_parser!(u32).range(..=i64::from(i32::MAX)))]
    source_reader_transaction_timeout_ms: Option<u32>,

    /// Maximum idle time in a source reader transaction in milliseconds.
    #[arg(hide_short_help = true, long, value_name = "MS", help_heading = "Timeouts", value_parser = clap::value_parser!(u32).range(..=i64::from(i32::MAX)))]
    source_reader_idle_in_transaction_timeout_ms: Option<u32>,

    /// Source disconnect check interval in milliseconds. Zero disables checks.
    #[arg(hide_short_help = true, long, value_name = "MS", help_heading = "Timeouts", value_parser = clap::value_parser!(u32).range(..=i64::from(i32::MAX)))]
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
