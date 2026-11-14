use std::fmt::Display;

use anyhow::{Context, Result, ensure};
use postgres_protocol::escape::escape_literal;
use tokio_postgres::types::{ToSql, Type};

use crate::predicate::{Predicate, parameters::parsing::ParameterValue};

pub(super) fn where_clause(condition: &str) -> String {
    format!(" WHERE (\n{condition}\n)")
}

pub(super) fn copy_condition(predicate: &Predicate) -> Result<String> {
    let mut condition = String::with_capacity(predicate.sql.len());
    let mut copied_end = 0;
    for parameter in &predicate.parameters {
        let span = &parameter.bind_span;
        ensure!(span.start < span.end, "Invalid bind marker range");
        condition.push_str(
            predicate
                .sql
                .get(copied_end..span.start)
                .context("Invalid bind marker range")?,
        );
        condition.push_str(&copy_literal(&parameter.value));
        copied_end = span.end;
    }
    condition.push_str(
        predicate
            .sql
            .get(copied_end..)
            .context("Invalid bind marker range")?,
    );
    Ok(condition)
}

pub(super) fn bind_value(value: &ParameterValue) -> (&(dyn ToSql + Sync), Type) {
    match value {
        ParameterValue::Text(value) => (value, Type::TEXT),
        ParameterValue::Integer(value) => (value, Type::INT4),
        ParameterValue::Bigint(value) => (value, Type::INT8),
        ParameterValue::Boolean(value) => (value, Type::BOOL),
        ParameterValue::Date(value) => (value, Type::DATE),
        ParameterValue::IntegerArray(value) => (value, Type::INT4_ARRAY),
        ParameterValue::BigintArray(value) => (value, Type::INT8_ARRAY),
    }
}

fn copy_literal(value: &ParameterValue) -> String {
    match value {
        ParameterValue::Text(value) => format!("({}::pg_catalog.text)", escape_literal(value)),
        ParameterValue::Integer(value) => format!("(({value})::pg_catalog.int4)"),
        ParameterValue::Bigint(value) => format!("(({value})::pg_catalog.int8)"),
        ParameterValue::Boolean(value) => format!("({value}::pg_catalog.bool)"),
        ParameterValue::Date(value) => {
            format!("({}::pg_catalog.date)", escape_literal(&value.to_string()))
        }
        ParameterValue::IntegerArray(value) => copy_array(value, "int4"),
        ParameterValue::BigintArray(value) => copy_array(value, "int8"),
    }
}

fn copy_array<T: Display>(items: &[T], element_type: &str) -> String {
    let items = items
        .iter()
        .map(T::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!("(ARRAY[{items}]::pg_catalog.{element_type}[])")
}
