use sqlparser::{
    dialect::PostgreSqlDialect,
    parser::{Parser, ParserError},
    tokenizer::{Location, Span, Token, TokenWithSpan},
};

use super::{error::PredicateParseError, tokenizer::source::SourceLocations};

pub(super) fn validate_expression(
    input: &str,
    tokens: Vec<TokenWithSpan>,
) -> Result<(), PredicateParseError> {
    let token_spans: Vec<_> = tokens.iter().map(|token| token.span).collect();
    let mut parser = Parser::new(&PostgreSqlDialect {}).with_tokens_with_locations(tokens);
    if parser.peek_token_ref().token == Token::EOF {
        return Err(PredicateParseError {
            message: "SQL condition is empty".into(),
            source_span: Some(input.len()..input.len()),
        });
    }
    parser
        .parse_expr()
        .map_err(|error| syntax_error(error, input, &token_spans))?;
    let next = parser.peek_token_ref();
    if next.token != Token::EOF {
        return Err(PredicateParseError {
            message: format!(
                "Expected the end of a single SQL condition, found {}",
                next.token
            ),
            source_span: SourceLocations::new(input).span(next.span),
        });
    }
    Ok(())
}

fn syntax_error(error: ParserError, input: &str, token_spans: &[Span]) -> PredicateParseError {
    let mut message = match error {
        ParserError::ParserError(message) | ParserError::TokenizerError(message) => message,
        ParserError::RecursionLimitExceeded => error.to_string(),
    };
    // sqlparser includes syntax error locations in the message. Use the token
    // at that location to report its complete range.
    let source_span = match error_location(&message) {
        Some((message_end, location)) => {
            message.truncate(message_end);
            let source = SourceLocations::new(input);
            token_spans
                .iter()
                .find(|span| span.start == location)
                .and_then(|&span| source.span(span))
                .or_else(|| source.point(location))
        }
        None if message.ends_with("found: EOF") => Some(input.len()..input.len()),
        None => None,
    };
    PredicateParseError {
        message,
        source_span,
    }
}

fn error_location(message: &str) -> Option<(usize, Location)> {
    let (reason, suffix) = message.rsplit_once(" at Line: ")?;
    let (line, column) = suffix.split_once(", Column: ")?;
    Some((
        reason.len(),
        Location {
            line: line.parse().ok()?,
            column: column.parse().ok()?,
        },
    ))
}
