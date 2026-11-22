use crate::table::QualifiedTable;

use super::dependencies::ObjectAddress;

#[derive(Debug, Eq, PartialEq)]
pub struct ConstraintDefinition {
    pub table: QualifiedTable,
    pub name: String,
    pub kind: ConstraintKind,
    /// The constraint body, including its options, returned by PostgreSQL.
    pub definition: String,
    pub validated: bool,
    pub parent: Option<ObjectAddress>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ConstraintKind {
    Check,
    PrimaryKey {
        index: ObjectAddress,
    },
    Unique {
        index: ObjectAddress,
    },
    Exclusion {
        index: ObjectAddress,
    },
    ForeignKey {
        referenced_table: QualifiedTable,
        referenced_index: ObjectAddress,
    },
}
