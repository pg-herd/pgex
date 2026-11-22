use crate::table::QualifiedTable;

use super::{
    constraints::ConstraintDefinition,
    dependencies::{ObjectAddress, SchemaDependency},
    indexes::IndexDefinition,
    relations::{SequenceDefinition, TableDefinition, ViewDefinition},
    types::TypeDefinition,
};

/// The definitions and raw catalog dependencies read from a source database.
#[derive(Debug, Eq, PartialEq)]
pub struct SourceSchema {
    pub objects: Vec<SchemaObject>,
    pub dependencies: Vec<SchemaDependency>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct SchemaObject {
    pub address: ObjectAddress,
    pub definition: SchemaDefinition,
}

#[derive(Debug, Eq, PartialEq)]
pub enum SchemaDefinition {
    Namespace(NamespaceDefinition),
    Extension(ExtensionDefinition),
    Type(TypeDefinition),
    Table(TableDefinition),
    Sequence(SequenceDefinition),
    Routine(RoutineDefinition),
    View(ViewDefinition),
    MaterializedView(ViewDefinition),
    Index(IndexDefinition),
    Constraint(ConstraintDefinition),
    Trigger(TriggerDefinition),
}

#[derive(Debug, Eq, PartialEq)]
pub struct QualifiedName {
    pub schema: String,
    pub name: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct NamespaceDefinition {
    pub name: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ExtensionDefinition {
    pub name: String,
    pub schema: String,
    pub version: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct RoutineDefinition {
    pub name: QualifiedName,
    pub identity_arguments: String,
    /// The complete definition returned by PostgreSQL.
    pub definition: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct TriggerDefinition {
    pub table: QualifiedTable,
    pub name: String,
    pub definition: String,
    pub enabled: TriggerEnabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerEnabled {
    Origin,
    Disabled,
    Replica,
    Always,
}
