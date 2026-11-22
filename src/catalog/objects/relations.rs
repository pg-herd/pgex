use crate::table::QualifiedTable;

use super::inventory::QualifiedName;

#[derive(Debug, Eq, PartialEq)]
pub struct TableDefinition {
    pub name: QualifiedTable,
    pub persistence: TablePersistence,
    pub columns: Vec<ColumnDefinition>,
    pub inherited_from: Vec<QualifiedTable>,
    pub partition: Option<TablePartition>,
    pub partition_key: Option<String>,
    pub storage_options: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TablePersistence {
    Permanent,
    Unlogged,
}

#[derive(Debug, Eq, PartialEq)]
pub struct TablePartition {
    pub parent: QualifiedTable,
    pub bound: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ColumnDefinition {
    pub name: String,
    /// The PostgreSQL type as SQL, including modifiers such as length or precision.
    pub data_type: String,
    pub collation: Option<QualifiedName>,
    pub nullable: bool,
    pub value: Option<ColumnValue>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ColumnValue {
    Default(String),
    Generated {
        expression: String,
        storage: GeneratedStorage,
    },
    Identity {
        generation: IdentityGeneration,
        sequence: QualifiedName,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedStorage {
    Stored,
    Virtual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityGeneration {
    Always,
    ByDefault,
}

#[derive(Debug, Eq, PartialEq)]
pub struct SequenceDefinition {
    pub name: QualifiedName,
    pub data_type: String,
    pub start: i64,
    pub increment: i64,
    pub min: i64,
    pub max: i64,
    pub cache: i64,
    pub cycle: bool,
    pub owned_by: Option<SequenceOwner>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct SequenceOwner {
    pub table: QualifiedTable,
    pub column: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ViewDefinition {
    pub name: QualifiedName,
    pub query: String,
    pub options: Vec<String>,
    pub check_option: Option<ViewCheckOption>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViewCheckOption {
    Local,
    Cascaded,
}
