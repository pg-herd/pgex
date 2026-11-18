use std::ops::Range;

use super::parameters::parsing::ParameterValue;

#[derive(Clone)]
pub struct Predicate {
    pub sql: String,
    /// Entries are in PostgreSQL bind-index order.
    pub parameters: Vec<BoundParameter>,
}

#[derive(Clone)]
pub struct BoundParameter {
    pub bind_span: Range<usize>,
    pub value: ParameterValue,
}
