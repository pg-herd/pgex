/// Identifies an object, or a column of an object, in the source database.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ObjectAddress {
    pub class_oid: u32,
    pub object_oid: u32,
    pub sub_id: i32,
}

#[derive(Debug, Eq, PartialEq)]
pub struct SchemaDependency {
    pub dependent: ObjectAddress,
    pub referenced: ObjectAddress,
    pub kind: DependencyKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DependencyKind {
    Normal,
    Auto,
    Internal,
    PartitionPrimary,
    PartitionSecondary,
    Extension,
    AutoExtension,
}
