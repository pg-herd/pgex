use super::{
    syntax,
    tokenizer::tokens::{self, TokenizedPredicate},
};

pub use super::{
    error::PredicateParseError,
    placeholders::{ParameterType, Placeholder, is_parameter_name},
};

/// A SQL condition parsed without supplying parameter values.
#[derive(Debug)]
pub struct ParsedPredicate {
    /// PostgreSQL condition using numbered bind markers.
    pub sql: String,
    /// One entry per occurrence, in SQL order.
    pub placeholders: Vec<Placeholder>,
}

pub fn parse_predicate(input: &str) -> Result<ParsedPredicate, PredicateParseError> {
    let TokenizedPredicate { predicate, tokens } = tokens::parse(input)?;
    syntax::validate_expression(input, tokens)?;
    Ok(predicate)
}
