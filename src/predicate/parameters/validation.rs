use super::{super::parser::Placeholder, parsing::parse_parameter_value};

pub use super::{
    error::PlaceholderValidationError,
    values::{ParameterInput, ParameterOrigin},
};

/// Checks that each placeholder has a supplied value compatible with its declared type.
pub fn validate_placeholder_values<'a>(
    placeholders: &'a [Placeholder],
    inputs: &[ParameterInput],
) -> Result<(), PlaceholderValidationError<'a>> {
    for placeholder in placeholders {
        let input = inputs
            .iter()
            .find(|input| input.name == placeholder.name)
            .ok_or(PlaceholderValidationError::MissingValue { placeholder })?;
        if let Some(expected) = placeholder.declared_type {
            parse_parameter_value(&input.raw_value, expected).map_err(|reason| {
                PlaceholderValidationError::InvalidValue {
                    placeholder,
                    reason,
                }
            })?;
        }
    }
    Ok(())
}
