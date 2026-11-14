use std::ops::Range;

pub fn is_parameter_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterType {
    Text,
    Integer,
    Bigint,
    Boolean,
    Date,
    IntegerArray,
    BigintArray,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Placeholder {
    /// One-based PostgreSQL bind index for this occurrence (`1` means `$1`).
    pub bind_index: usize,
    pub name: String,
    pub declared_type: Option<ParameterType>,
    /// Byte range of the complete placeholder in the original condition.
    pub source_span: Range<usize>,
    /// Byte range of the generated bind marker in `ParsedPredicate::sql`.
    pub bind_span: Range<usize>,
}
