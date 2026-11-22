use crate::catalog::objects::{
    constraints::ConstraintDefinition,
    dependencies::ObjectAddress,
    indexes::{IndexDefinition, QualifiedIndexName},
    inventory::SchemaObject,
};

#[derive(Debug, Eq, PartialEq)]
pub struct CloneSchemaPlan {
    pub pre_copy: PreCopySchema,
    pub indexes: IndexPlan,
    /// Constraints to create after the index builds.
    pub constraints: Vec<ConstraintDefinition>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct PreCopySchema {
    pub objects: Vec<SchemaObject>,
    /// Dependencies between objects in this phase.
    pub dependencies: Vec<CreationDependency>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct CreationDependency {
    pub object: ObjectAddress,
    pub requires: ObjectAddress,
}

#[derive(Debug, Eq, PartialEq)]
pub struct IndexPlan {
    pub builds: Vec<IndexBuild>,
    /// Attachments in dependency order, applied after the builds.
    pub partition_attachments: Vec<PartitionIndexAttachment>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum IndexBuild {
    Standalone(IndexDefinition),
    ConstraintOwned {
        index: IndexDefinition,
        constraint: ConstraintDefinition,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub struct PartitionIndexAttachment {
    pub parent: QualifiedIndexName,
    pub child: QualifiedIndexName,
}
