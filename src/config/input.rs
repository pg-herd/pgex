use std::{
    num::{NonZeroU32, NonZeroUsize},
    path::PathBuf,
};

use crate::{
    predicate::{parameters::validation::ParameterInput, parser::ParsedPredicate},
    table::QualifiedTable,
};

use super::snapshot_mode::SnapshotMode;

#[derive(Default)]
pub struct CliInput {
    pub settings: ConfigInput,
}

pub struct FileConfig {
    pub settings: ConfigInput,
}

/// Parsed settings. An absent scalar or list leaves precedence to the other input.
#[derive(Default)]
pub struct ConfigInput {
    pub source: SourceInput,
    pub target: PostgresTargetInput,
    pub params: Vec<ParameterInput>,
}

#[derive(Default)]
pub struct SourceInput {
    pub connection: ConnectionInput,
    pub session: SourceSessionInput,
    pub snapshot: SnapshotInput,
    pub selection: SelectionInput,
}

#[derive(Default)]
pub struct ConnectionInput {
    /// PostgreSQL URL or the legacy keyword/value spelling accepted by the CLI.
    pub url: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub ca_file: Option<PathBuf>,
}

#[derive(Default)]
pub struct SourceSessionInput {
    pub row_security_off: Option<bool>,
}

#[derive(Default)]
pub struct SnapshotInput {
    pub mode: Option<SnapshotMode>,
    pub reader_connection_limit: Option<NonZeroUsize>,
    pub client_connection_check_interval_ms: Option<u32>,
    pub exporter_timeouts: TimeoutInput,
    pub reader_timeouts: TimeoutInput,
}

#[derive(Default)]
pub struct TimeoutInput {
    pub transaction_ms: Option<u32>,
    pub idle_in_transaction_ms: Option<u32>,
}

#[derive(Default)]
pub struct SelectionInput {
    pub tables: Option<Vec<QualifiedTable>>,
    pub shared: Option<PredicateInput>,
    pub table_rules: Option<Vec<ColumnRuleInput>>,
    pub table_filters: Option<Vec<TableFilterInput>>,
}

pub struct PredicateInput {
    pub original: String,
    pub parsed: ParsedPredicate,
    pub origin: PredicateOrigin,
}

pub enum PredicateOrigin {
    Cli { flag: String },
    Yaml { file: PathBuf, field: String },
}

pub struct ColumnRuleInput {
    pub has_column: String,
    pub condition: PredicateInput,
}

pub struct TableFilterInput {
    pub table: QualifiedTable,
    pub condition: PredicateInput,
}

#[derive(Default)]
pub struct PostgresTargetInput {
    pub connection: ConnectionInput,
    pub session: PostgresSessionInput,
    pub transfer: PostgresTransferInput,
    pub delete_filters: Option<Vec<TableFilterInput>>,
}

#[derive(Default)]
pub struct PostgresSessionInput {
    pub row_security_off: Option<bool>,
    pub replica_role: Option<bool>,
}

#[derive(Default)]
pub struct PostgresTransferInput {
    pub workers: Option<NonZeroUsize>,
    pub commit_group_mib: Option<NonZeroU32>,
    pub validate_layouts: Option<bool>,
    pub no_locks: Option<bool>,
    pub skip_delete: Option<bool>,
    pub truncate: Option<bool>,
}
