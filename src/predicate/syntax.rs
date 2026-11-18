use std::ops::Range;

use super::{
    error::PredicateParseError,
    libpg_query::{self, Token, TokenKind},
};

struct MappedToken {
    validation_span: Range<usize>,
    source_span: Range<usize>,
    kind: TokenKind,
}

pub(super) fn validate_expression(
    input: &str,
    tokens: Vec<Token>,
) -> Result<(), PredicateParseError> {
    if tokens.iter().all(|token| token.kind == TokenKind::Trivia) {
        return Err(PredicateParseError {
            message: "SQL condition is empty".into(),
            source_span: Some(input.len()..input.len()),
        });
    }
    let mut expression = String::with_capacity(input.len());
    let mut mappings = Vec::with_capacity(tokens.len());
    let mut copied_end = 0;
    let mut depth = 0;
    let mut unmatched = None;
    for token in tokens {
        let source_span = token.source_span;
        expression.push_str(&input[copied_end..source_span.start]);
        let start = expression.len();
        if let TokenKind::Parameter(marker) = &token.kind {
            // The inserted spaces separate markers from adjacent identifiers;
            // this entire segment still maps to the original placeholder.
            expression.push(' ');
            expression.push_str(marker);
            expression.push(' ');
        } else {
            expression.push_str(&input[source_span.clone()]);
        }
        match token.kind {
            TokenKind::Symbol('(') => depth += 1,
            TokenKind::Symbol(')') if depth == 0 => {
                unmatched.get_or_insert(mappings.len());
            }
            TokenKind::Symbol(')') => depth -= 1,
            _ => {}
        }
        copied_end = source_span.end;
        mappings.push(MappedToken {
            validation_span: start..expression.len(),
            source_span,
            kind: token.kind,
        });
    }
    expression.push_str(&input[copied_end..]);
    match libpg_query::parse_expression(&expression) {
        Ok(()) => match unmatched {
            Some(index) => Err(unexpected_token(input, &mappings, index)),
            None => Ok(()),
        },
        Err(libpg_query::ParseError {
            message,
            byte_offset,
        }) => {
            let Some(position) = byte_offset else {
                return Err(PredicateParseError {
                    message,
                    source_span: None,
                });
            };
            let token = mappings
                .iter()
                .position(|token| token.validation_span.contains(&position));
            // An unmatched closing token is an earlier expression error than
            // an error reported later by the backend, including at EOF.
            if let Some(index) = unmatched
                && mappings[index].validation_span.start <= position
            {
                return Err(unexpected_token(input, &mappings, index));
            }
            let at_end = position == expression.len();
            let source_span = if at_end {
                Some(input.len()..input.len())
            } else {
                token.map(|index| mappings[index].source_span.clone())
            };
            let message = if at_end
                && needs_expression(
                    input,
                    mappings
                        .iter()
                        .rposition(|token| token.kind != TokenKind::Trivia)
                        .map(|index| &mappings[index]),
                ) {
                "Expected: an expression, found: EOF".into()
            } else {
                message
            };
            Err(PredicateParseError {
                message,
                source_span,
            })
        }
    }
}

fn needs_expression(input: &str, token: Option<&MappedToken>) -> bool {
    let Some(token) = token else {
        return false;
    };
    let raw = &input[token.source_span.clone()];
    match token.kind {
        TokenKind::Word => matches!(raw.to_ascii_uppercase().as_str(), "AND" | "OR" | "NOT"),
        TokenKind::Symbol(_) | TokenKind::Other => matches!(
            raw,
            "=" | "<>" | "!=" | "<" | ">" | "<=" | ">=" | "+" | "-" | "*" | "/"
        ),
        _ => false,
    }
}

fn unexpected_token(input: &str, tokens: &[MappedToken], index: usize) -> PredicateParseError {
    let previous = tokens[..index]
        .iter()
        .rev()
        .find(|token| token.kind != TokenKind::Trivia);
    let message = if needs_expression(input, previous) {
        "Expected: an expression, found: )".into()
    } else {
        "Expected the end of a single SQL condition, found )".into()
    };
    PredicateParseError {
        message,
        source_span: Some(tokens[index].source_span.clone()),
    }
}

#[cfg(test)]
mod tests {
    use crate::predicate::parser::parse_predicate;

    #[test]
    fn maps_repeated_closing_tokens_to_the_actual_failure() {
        let input = "note = '🌿' AND EXISTS (SELECT 1 WHERE id = {{id}}) AND )";
        let position = input.rfind(')').unwrap();
        assert_eq!(
            parse_predicate(input).unwrap_err().source_span,
            Some(position..position + 1)
        );
    }

    #[test]
    fn maps_backend_errors_after_many_markers_and_unicode() {
        let mut input = "note = '🌿'".to_owned();
        for number in 0..12 {
            input.push_str(&format!(" AND id = {{{{parameter_{number}:bigint}}}}"));
        }
        input.push_str(" alias_é");
        let error = parse_predicate(&input).unwrap_err();
        assert_eq!(&input[error.source_span.unwrap()], "alias_é");
    }

    #[test]
    fn cannot_escape_the_synthetic_policy_statement() {
        for input in [
            "TRUE) WITH CHECK (TRUE",
            "TRUE) USING (FALSE",
            "TRUE, FALSE",
            "SELECT 1",
        ] {
            assert!(parse_predicate(input).is_err(), "{input}");
        }
    }

    #[test]
    fn backend_rejects_an_incomplete_expression_with_original_eof() {
        let input = "note = '🌿' AND id = {{identifier:bigint}} AND\r\n";
        assert_eq!(
            parse_predicate(input).unwrap_err().source_span,
            Some(input.len()..input.len())
        );
    }
}
