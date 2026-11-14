use std::{
    num::{NonZeroU32, NonZeroUsize},
    path::PathBuf,
};

use crate::{predicate::Predicate, snapshot::SnapshotTimeouts, table::QualifiedTable};

pub struct CompiledConfig {
    pub source: SourceConfig,
    pub target: PostgresTargetConfig,
}

pub struct SourceConfig {
    pub connection: ConnectionConfig,
    pub session: SourceSessionSettings,
    pub snapshot: SnapshotSettings,
    pub selection: SelectionOptions,
}

pub struct ConnectionConfig {
    /// PostgreSQL URL or the legacy keyword/value spelling accepted by the CLI.
    pub url: String,
    pub user: Option<String>,
    pub password: Option<String>,
    pub ca_file: Option<PathBuf>,
}

pub struct SourceSessionSettings {
    pub row_security_off: bool,
}

pub struct SnapshotSettings {
    pub reader_connection_limit: NonZeroUsize,
    pub client_connection_check_interval_ms: u32,
    pub exporter_timeouts: SnapshotTimeouts,
    pub reader_timeouts: SnapshotTimeouts,
}

pub struct SelectionOptions {
    pub tables: TableSelection,
    pub filters: SourceFilterPolicy,
}

pub enum TableSelection {
    Discover,
    Explicit(Vec<QualifiedTable>),
}

pub struct SourceFilterPolicy {
    pub shared: Option<Predicate>,
    pub table_rules: Vec<ColumnRule>,
    pub table_filters: Vec<TableFilter>,
}

pub struct ColumnRule {
    pub has_column: String,
    pub condition: Predicate,
}

pub struct TableFilter {
    pub table: QualifiedTable,
    pub condition: Predicate,
}

pub struct PostgresTargetConfig {
    pub connection: ConnectionConfig,
    pub session: PostgresSessionSettings,
    pub transfer: PostgresTransferSettings,
    pub delete_filters: Vec<TableFilter>,
}

pub struct PostgresSessionSettings {
    pub row_security_off: bool,
    pub replica_role: bool,
}

pub struct PostgresTransferSettings {
    pub workers: NonZeroUsize,
    pub commit_group_mib: NonZeroU32,
    pub validate_layouts: bool,
    pub skip_delete: bool,
}
