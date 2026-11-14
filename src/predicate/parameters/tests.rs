use time::macros::date;

use super::parsing::{InvalidValueReason, ParameterValue, parse_parameter_value};
use super::validation::{
    ParameterInput, ParameterOrigin, PlaceholderValidationError, validate_placeholder_values,
};
use crate::predicate::parser::{ParameterType, parse_predicate};

#[test]
fn reports_the_placeholder_with_a_missing_value() {
    let sql = "created_at < {{cutoff:date}} AND status = {{state}}";
    let predicate = parse_predicate(sql).unwrap();
    let cutoff = input("cutoff", "2026-01-01");
    let error = validate_placeholder_values(&predicate.placeholders, std::slice::from_ref(&cutoff))
        .unwrap_err();
    let PlaceholderValidationError::MissingValue { placeholder } = error else {
        panic!("Expected a missing value error");
    };
    assert_eq!(placeholder.name, "state");
    assert_eq!(&sql[placeholder.source_span.clone()], "{{state}}");

    validate_placeholder_values(
        &predicate.placeholders,
        &[cutoff, input("state", "active"), input("other", "1")],
    )
    .unwrap();
}

#[test]
fn reports_the_placeholder_with_an_invalid_value() {
    let sql = "status = {{state}} AND created_at < {{cutoff:date}}";
    let predicate = parse_predicate(sql).unwrap();
    let error = validate_placeholder_values(
        &predicate.placeholders,
        &[input("state", "active"), input("cutoff", "2026-02-30")],
    )
    .unwrap_err();
    let PlaceholderValidationError::InvalidValue {
        placeholder,
        reason,
    } = error
    else {
        panic!("Expected an invalid value error");
    };
    assert_eq!(placeholder.name, "cutoff");
    assert_eq!(placeholder.declared_type, Some(ParameterType::Date));
    assert_eq!(&sql[placeholder.source_span.clone()], "{{cutoff:date}}");
    assert_eq!(reason, InvalidValueReason::InvalidType);
}

#[test]
fn parses_all_supported_parameter_types() {
    use ParameterType::{Bigint, BigintArray, Boolean, Date, Integer, IntegerArray, Text};

    for (kind, raw, expected) in [
        (
            Text,
            " O'Reilly 🌿 ",
            ParameterValue::Text(" O'Reilly 🌿 ".into()),
        ),
        (Integer, "-2147483648", ParameterValue::Integer(i32::MIN)),
        (
            Bigint,
            "9223372036854775807",
            ParameterValue::Bigint(i64::MAX),
        ),
        (Boolean, "false", ParameterValue::Boolean(false)),
        (
            Date,
            "2024-02-29",
            ParameterValue::Date(date!(2024 - 02 - 29)),
        ),
        (
            IntegerArray,
            "-1, 2",
            ParameterValue::IntegerArray(vec![-1, 2]),
        ),
        (
            BigintArray,
            "2147483648, 9223372036854775807",
            ParameterValue::BigintArray(vec![i64::from(i32::MAX) + 1, i64::MAX]),
        ),
    ] {
        assert_eq!(parse_parameter_value(raw, kind), Ok(expected));
    }
}

#[test]
fn rejects_invalid_and_out_of_range_typed_values() {
    use ParameterType::{Bigint, BigintArray, Boolean, Date, Integer, IntegerArray};

    for (kind, raw) in [
        (Integer, "2147483648"),
        (Bigint, "-9223372036854775809"),
        (Boolean, "yes"),
        (Date, "2023-02-29"),
        (Date, "2026-2-02"),
        (Date, "0000-01-01"),
        (IntegerArray, "1,,2"),
        (IntegerArray, "1,"),
        (IntegerArray, ""),
        (IntegerArray, "2147483648"),
        (BigintArray, "9223372036854775808"),
    ] {
        assert_eq!(
            parse_parameter_value(raw, kind),
            Err(InvalidValueReason::InvalidType),
            "{kind:?} and {raw:?}",
        );
    }
}

#[test]
fn rejects_nul_in_text_values() {
    assert_eq!(
        parse_parameter_value("bad\0text", ParameterType::Text),
        Err(InvalidValueReason::ContainsNul),
    );
}

#[test]
fn accepts_a_filter_with_no_parameters() {
    let predicate = parse_predicate("status = 'active'").unwrap();
    validate_placeholder_values(&predicate.placeholders, &[]).unwrap();
}

fn input(name: &str, raw_value: &str) -> ParameterInput {
    ParameterInput {
        name: name.into(),
        raw_value: raw_value.into(),
        origin: ParameterOrigin::Cli,
    }
}
