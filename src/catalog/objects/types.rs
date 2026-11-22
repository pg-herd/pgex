use super::inventory::QualifiedName;

#[derive(Debug, Eq, PartialEq)]
pub struct TypeDefinition {
    pub name: QualifiedName,
    pub kind: TypeKind,
}

#[derive(Debug, Eq, PartialEq)]
pub enum TypeKind {
    Enum {
        labels: Vec<String>,
    },
    Domain {
        base_type: String,
        collation: Option<QualifiedName>,
        default: Option<String>,
        not_null: bool,
        constraints: Vec<DomainConstraintDefinition>,
    },
    Composite {
        fields: Vec<CompositeField>,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub struct DomainConstraintDefinition {
    pub name: String,
    pub definition: String,
    pub validated: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub struct CompositeField {
    pub name: String,
    pub data_type: String,
    pub collation: Option<QualifiedName>,
}
