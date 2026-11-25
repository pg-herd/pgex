use std::ffi::CString;

use super::{
    error::{ParseError, copy_error},
    ffi::{pg_query_free_split_result, pg_query_split_with_parser},
};

// Link the native PostgreSQL parser built by pg_query.
use pg_query as _;

const PREFIX: &str = "CREATE POLICY pgex_predicate ON pgex_table USING (\n";

/// Parse an expression offline using PostgreSQL's grammar. The predicate
/// validator also rejects unmatched closing parentheses, so a condition cannot
/// escape the surrounding USING clause.
pub(in crate::predicate) fn parse_expression(expression: &str) -> Result<(), ParseError> {
    let statement = format!("{PREFIX}{expression}\n)");
    let statement = CString::new(statement).map_err(|error| ParseError {
        message: "SQL condition contains a NUL character".into(),
        byte_offset: Some(error.nul_position() - PREFIX.len()),
    })?;
    let result = unsafe { pg_query_split_with_parser(statement.as_ptr()) };
    let error = if result.error.is_null() {
        None
    } else {
        let mut error = unsafe {
            copy_error(
                statement.to_str().expect("Expression is UTF-8"),
                &*result.error,
            )
        };
        error.byte_offset = error
            .byte_offset
            .and_then(|position| position.checked_sub(PREFIX.len()))
            .map(|position| position.min(expression.len()));
        Some(error)
    };
    // The split result owns the statement spans and native diagnostics.
    unsafe { pg_query_free_split_result(result) };
    error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::parse_expression;

    #[test]
    fn errors_use_expression_byte_offsets_after_unicode_and_crlf() {
        let input = "note = '🌿'\r\nAND id = LIMIT";
        let error = parse_expression(input).expect_err("Invalid expression");
        assert_eq!(error.byte_offset, input.find("LIMIT"));
        assert_eq!(error.message, "syntax error at or near \"LIMIT\"");
    }

    #[test]
    fn rejects_nul_at_its_expression_byte_offset() {
        let input = "x = '\0'";
        let error = parse_expression(input).expect_err("NUL is not valid SQL text");
        assert_eq!(error.byte_offset, input.find('\0'));
    }

    #[test]
    fn missing_expression_points_to_expression_eof() {
        let input = "note = '🌿' AND id =\r\n";
        let error = parse_expression(input).expect_err("Incomplete expression");
        assert_eq!(error.byte_offset, Some(input.len()));
        assert_eq!(error.message, "syntax error at or near \")\"");
    }
}
