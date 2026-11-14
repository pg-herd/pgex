use anyhow::{Result, anyhow, bail};

use crate::predicate::{
    Predicate,
    error_format::format_predicate_diagnostic,
    parameters::{
        parsing::{InvalidValueReason, bind_parameters},
        validation::PlaceholderValidationError,
    },
    parser::{ParameterType, Placeholder},
};

use super::super::input::{PredicateInput, PredicateOrigin};
use super::merge::{Parameters, value_origin};

pub(super) fn compile_predicate(
    input: PredicateInput,
    params: &mut Parameters<'_>,
) -> Result<Predicate> {
    let parameters = bind_parameters(&input.parsed.placeholders, |name| {
        let (&name, supplied) = params.values.get_key_value(name)?;
        params.used.insert(name);
        Some(supplied.raw_value.as_str())
    })
    .map_err(|error| format_binding_error(&input, params, error))?;
    Ok(Predicate {
        sql: input.parsed.sql,
        parameters,
    })
}

pub(super) fn reject_unused_parameters(params: &Parameters<'_>) -> Result<()> {
    if let Some((&name, input)) = params
        .values
        .iter()
        .filter(|(name, _)| !params.used.contains(**name))
        .min_by_key(|(name, _)| *name)
    {
        bail!(
            "Unused parameter '{name}' from {}",
            value_origin(&input.origin)
        );
    }
    Ok(())
}

fn format_binding_error(
    input: &PredicateInput,
    params: &Parameters<'_>,
    error: PlaceholderValidationError<'_>,
) -> anyhow::Error {
    match error {
        PlaceholderValidationError::MissingValue { placeholder } => predicate_error(
            input,
            placeholder,
            &format!("Missing parameter '{}'", placeholder.name),
        ),
        PlaceholderValidationError::InvalidValue {
            placeholder,
            reason,
        } => {
            let name = placeholder.name.as_str();
            let supplied = params.values[name];
            let kind = placeholder.declared_type.unwrap_or(ParameterType::Text);
            let problem = match reason {
                InvalidValueReason::ContainsNul => "contains a NUL character".to_owned(),
                InvalidValueReason::InvalidType => format!("must be a valid {}", type_name(kind)),
            };
            let message = format!(
                "Parameter '{name}' from {} {problem}",
                value_origin(&supplied.origin)
            );
            predicate_error(input, placeholder, &message)
        }
    }
}

fn predicate_error(input: &PredicateInput, slot: &Placeholder, message: &str) -> anyhow::Error {
    let origin = match &input.origin {
        PredicateOrigin::Cli { flag } => flag.clone(),
        PredicateOrigin::Yaml { file, field } => format!("{} ({field})", file.display()),
    };
    let detail = format_predicate_diagnostic(&input.original, message, Some(&slot.source_span));
    anyhow!("{origin}\n{detail}")
}

fn type_name(kind: ParameterType) -> &'static str {
    match kind {
        ParameterType::Text => "text value",
        ParameterType::Integer => "integer",
        ParameterType::Bigint => "bigint",
        ParameterType::Boolean => "boolean",
        ParameterType::Date => "date (YYYY-MM-DD)",
        ParameterType::IntegerArray => "integer array",
        ParameterType::BigintArray => "bigint array",
    }
}
