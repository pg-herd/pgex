use anyhow::{Result, bail, ensure};

use super::super::{
    libpg_query::{Token, TokenKind},
    placeholders::{ParameterType, is_parameter_name},
};

pub(super) fn parse_body(input: &str, body: &[Token]) -> Result<(String, Option<ParameterType>)> {
    let [name, annotation @ ..] = body else {
        bail!("Expected a parameter name");
    };
    ensure!(
        matches!(name.kind, TokenKind::Word | TokenKind::QuotedIdentifier),
        "Expected an unquoted parameter name"
    );
    let value = &input[name.source_span.clone()];
    ensure!(
        name.kind == TokenKind::Word && is_parameter_name(value),
        "Expected an ASCII parameter name"
    );
    let declared_type = match annotation {
        [] => None,
        [colon, kind, suffix @ ..] if colon.kind == TokenKind::Symbol(':') => {
            Some(parse_type(input, kind, suffix)?)
        }
        _ => bail!("Expected a compact name and optional type annotation"),
    };
    Ok((value.into(), declared_type))
}

fn parse_type(input: &str, kind: &Token, suffix: &[Token]) -> Result<ParameterType> {
    ensure!(
        kind.kind == TokenKind::Word,
        "Expected an unquoted parameter type"
    );
    let is_array = matches!(suffix, [open, close] if open.kind == TokenKind::Symbol('[') && close.kind == TokenKind::Symbol(']'));
    ensure!(
        suffix.is_empty() || is_array,
        "Expected an optional [] suffix"
    );
    match (&input[kind.source_span.clone()], is_array) {
        ("text", false) => Ok(ParameterType::Text),
        ("integer", false) => Ok(ParameterType::Integer),
        ("bigint", false) => Ok(ParameterType::Bigint),
        ("boolean", false) => Ok(ParameterType::Boolean),
        ("date", false) => Ok(ParameterType::Date),
        ("integer", true) => Ok(ParameterType::IntegerArray),
        ("bigint", true) => Ok(ParameterType::BigintArray),
        _ => bail!("Unsupported parameter type"),
    }
}
