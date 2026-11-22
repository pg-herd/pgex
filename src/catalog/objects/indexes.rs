use crate::table::QualifiedTable;

use super::dependencies::ObjectAddress;

#[derive(Debug, Eq, PartialEq)]
pub struct QualifiedIndexName {
    pub schema: String,
    pub name: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct IndexDefinition {
    pub name: QualifiedIndexName,
    pub table: QualifiedTable,
    /// The index definition returned by PostgreSQL.
    pub definition: String,
    pub valid: bool,
    pub parent: Option<ObjectAddress>,
}
