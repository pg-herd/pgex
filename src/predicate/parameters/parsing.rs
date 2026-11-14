use std::str::FromStr;

use time::{Date, macros::format_description};

use super::{
    super::{
        BoundParameter,
        parser::{ParameterType, Placeholder},
    },
    error::PlaceholderValidationError,
};

pub use super::{error::InvalidValueReason, values::ParameterValue};

pub(crate) fn bind_parameters<'a, 'value>(
    placeholders: &'a [Placeholder],
    mut value_for: impl FnMut(&str) -> Option<&'value str>,
) -> Result<Vec<BoundParameter>, PlaceholderValidationError<'a>> {
    placeholders
        .iter()
        .map(|placeholder| {
            let raw = value_for(&placeholder.name)
                .ok_or(PlaceholderValidationError::MissingValue { placeholder })?;
            let kind = placeholder.declared_type.unwrap_or(ParameterType::Text);
            let value = parse_parameter_value(raw, kind).map_err(|reason| {
                PlaceholderValidationError::InvalidValue {
                    placeholder,
                    reason,
                }
            })?;
            Ok(BoundParameter {
                bind_span: placeholder.bind_span.clone(),
                value,
            })
        })
        .collect()
}

/// Parses one supplied value using the selected type.
pub fn parse_parameter_value(
    raw: &str,
    expected: ParameterType,
) -> Result<ParameterValue, InvalidValueReason> {
    if raw.contains('\0') {
        return Err(InvalidValueReason::ContainsNul);
    }
    match expected {
        ParameterType::Text => Ok(ParameterValue::Text(raw.into())),
        ParameterType::Integer => parse_scalar(raw).map(ParameterValue::Integer),
        ParameterType::Bigint => parse_scalar(raw).map(ParameterValue::Bigint),
        ParameterType::Boolean => parse_scalar(raw).map(ParameterValue::Boolean),
        ParameterType::Date => parse_date(raw)
            .map(ParameterValue::Date)
            .ok_or(InvalidValueReason::InvalidType),
        ParameterType::IntegerArray => parse_array(raw).map(ParameterValue::IntegerArray),
        ParameterType::BigintArray => parse_array(raw).map(ParameterValue::BigintArray),
    }
}

fn parse_scalar<T: FromStr>(raw: &str) -> Result<T, InvalidValueReason> {
    raw.parse().map_err(|_| InvalidValueReason::InvalidType)
}

fn parse_array<T: FromStr>(raw: &str) -> Result<Vec<T>, InvalidValueReason> {
    raw.split(',')
        .map(|item| parse_scalar(item.trim()))
        .collect()
}

fn parse_date(raw: &str) -> Option<Date> {
    if raw.len() != 10 || !raw.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let date = Date::parse(raw, format_description!("[year]-[month]-[day]")).ok()?;
    (date.year() > 0).then_some(date)
}
