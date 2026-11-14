use super::super::parser::Placeholder;

#[derive(Debug, Eq, PartialEq)]
pub enum InvalidValueReason {
    ContainsNul,
    InvalidType,
}

#[derive(Debug, Eq, PartialEq)]
pub enum PlaceholderValidationError<'a> {
    MissingValue {
        placeholder: &'a Placeholder,
    },
    InvalidValue {
        placeholder: &'a Placeholder,
        reason: InvalidValueReason,
    },
}
