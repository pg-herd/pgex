use sqlparser::{
    dialect::{Dialect, PostgreSqlDialect},
    tokenizer::{Token, TokenWithSpan, Tokenizer},
};

use super::{
    super::{error::PredicateParseError, parser::ParsedPredicate, placeholders::Placeholder},
    placeholder::parse_body,
    source::SourceLocations,
};

pub(in crate::predicate) struct TokenizedPredicate {
    pub predicate: ParsedPredicate,
    pub tokens: Vec<TokenWithSpan>,
}

pub(in crate::predicate) fn parse(input: &str) -> Result<TokenizedPredicate, PredicateParseError> {
    if let Some(offset) = input.find('\0') {
        return Err(PredicateParseError {
            message: "SQL condition contains a NUL character".into(),
            source_span: Some(offset..offset + 1),
        });
    }
    let source = SourceLocations::new(input);
    let dialect = PostgreSqlDialect {};
    let mut tokens = Tokenizer::new(&dialect, input)
        .tokenize_with_location()
        .map_err(|error| PredicateParseError {
            message: error.message,
            source_span: source.point(error.location),
        })?
        .into_iter();
    let mut expression_tokens = Vec::with_capacity(tokens.len());
    let mut placeholders = Vec::new();
    let mut sql = String::with_capacity(input.len());
    let mut copied_end = 0;

    while let Some(token) = tokens.next() {
        let start = token.span.start;
        let rejection = match token.token {
            Token::SemiColon => Some("Statement separator is not allowed"),
            Token::Placeholder(_) => Some("Use a named template parameter"),
            _ => None,
        };
        if let Some(message) = rejection {
            return Err(PredicateParseError {
                message: message.into(),
                source_span: source.span(token.span),
            });
        }
        let remaining = tokens.as_slice();
        if token.token != Token::LBrace
            || !remaining.first().is_some_and(|next| {
                next.token == Token::LBrace && next.span.start == token.span.end
            })
        {
            expression_tokens.push(token);
            continue;
        }

        let closing_index = closing_brace_index(remaining).ok_or_else(|| PredicateParseError {
            message: "Invalid placeholder: Expected two closing braces".into(),
            source_span: source.span(start.span_to(remaining[0].span.end)),
        })?;
        let original_span = start.span_to(remaining[closing_index].span.end);
        let source_span = source
            .span(original_span)
            .expect("Bind token has an original source span");
        let (name, declared_type) =
            parse_body(&remaining[1..closing_index - 1]).map_err(|error| PredicateParseError {
                message: format!("Invalid placeholder: {error}"),
                source_span: Some(source_span.clone()),
            })?;
        let _ = tokens.nth(closing_index);
        let bind_index = placeholders.len() + 1;
        let marker = format!("${bind_index}");
        sql.push_str(&input[copied_end..source_span.start]);
        // Braces separate tokens in the input, but a bind marker may need whitespace.
        if sql.ends_with(|ch| dialect.is_identifier_part(ch)) {
            sql.push(' ');
        }
        let bind_start = sql.len();
        sql.push_str(&marker);
        let bind_span = bind_start..sql.len();
        copied_end = source_span.end;
        if input[copied_end..].starts_with(|ch| dialect.is_identifier_part(ch)) {
            sql.push(' ');
        }
        placeholders.push(Placeholder {
            bind_index,
            name,
            declared_type,
            source_span,
            bind_span,
        });
        expression_tokens.push(TokenWithSpan {
            token: Token::Placeholder(marker),
            span: original_span,
        });
    }
    sql.push_str(&input[copied_end..]);
    Ok(TokenizedPredicate {
        predicate: ParsedPredicate { sql, placeholders },
        tokens: expression_tokens,
    })
}

// The slice starts at the second opening brace.
fn closing_brace_index(tokens: &[TokenWithSpan]) -> Option<usize> {
    let close = tokens
        .iter()
        .position(|token| token.token == Token::RBrace)?;
    let end = close + 1;
    (tokens.get(end)?.token == Token::RBrace).then_some(end)
}

#[cfg(test)]
mod tests {
    use super::{SourceLocations, Token, TokenizedPredicate, parse};

    #[test]
    fn substitutes_placeholders_before_expression_validation() {
        let parsed = parse("x = {{id}} AND").unwrap().predicate;
        assert_eq!(parsed.sql, "x = $1 AND");
    }

    #[test]
    fn numbered_tokens_keep_the_original_placeholder_ranges() {
        let input = "x = {{id:bigint}} OR (y = {{id}} AND z = {{other}})";
        let TokenizedPredicate { predicate, tokens } = parse(input).unwrap();
        let source = SourceLocations::new(input);
        let bind_tokens: Vec<_> = tokens
            .iter()
            .filter_map(|token| match &token.token {
                Token::Placeholder(marker) => {
                    let span = source.span(token.span).unwrap();
                    Some((marker.as_str(), &input[span]))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            bind_tokens,
            [
                ("$1", "{{id:bigint}}"),
                ("$2", "{{id}}"),
                ("$3", "{{other}}")
            ]
        );
        let actual: Vec<_> = predicate
            .placeholders
            .iter()
            .map(|slot| {
                (
                    slot.bind_index,
                    slot.name.as_str(),
                    &predicate.sql[slot.bind_span.clone()],
                )
            })
            .collect();
        assert_eq!(
            actual,
            [(1, "id", "$1"), (2, "id", "$2"), (3, "other", "$3")]
        );
    }
}
