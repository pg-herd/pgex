use anyhow::{Result, bail, ensure};
use sqlparser::tokenizer::{Token, TokenWithSpan};

use super::super::placeholders::{ParameterType, is_parameter_name};

pub(super) fn parse_body(body: &[TokenWithSpan]) -> Result<(String, Option<ParameterType>)> {
    let [name, annotation @ ..] = body else {
        bail!("Expected a parameter name");
    };
    let Token::Word(name) = &name.token else {
        bail!("Expected an unquoted parameter name");
    };
    ensure!(
        name.quote_style.is_none() && is_parameter_name(&name.value),
        "Expected an ASCII parameter name"
    );

    let declared_type = match annotation {
        [] => None,
        [colon, kind, suffix @ ..] if colon.token == Token::Colon => {
            Some(parse_type(&kind.token, suffix)?)
        }
        _ => bail!("Expected a compact name and optional type annotation"),
    };
    Ok((name.value.clone(), declared_type))
}

fn parse_type(kind: &Token, suffix: &[TokenWithSpan]) -> Result<ParameterType> {
    let Token::Word(kind) = kind else {
        bail!("Expected an unquoted parameter type");
    };
    ensure!(
        kind.quote_style.is_none(),
        "Expected an unquoted parameter type"
    );
    let is_array = matches!(suffix, [open, close] if open.token == Token::LBracket && close.token == Token::RBracket);
    ensure!(
        suffix.is_empty() || is_array,
        "Expected an optional [] suffix"
    );
    match (kind.value.as_str(), is_array) {
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
