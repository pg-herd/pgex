use std::path::PathBuf;

use time::Date;

/// A named raw value and its input origin.
pub struct ParameterInput {
    pub name: String,
    pub raw_value: String,
    pub origin: ParameterOrigin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParameterOrigin {
    Cli,
    Yaml { file: PathBuf },
}

#[derive(Debug, Eq, PartialEq)]
pub enum ParameterValue {
    Text(String),
    Integer(i32),
    Bigint(i64),
    Boolean(bool),
    Date(Date),
    IntegerArray(Vec<i32>),
    BigintArray(Vec<i64>),
}
