use std::borrow::Cow;

use super::{
    super::{
        error::PredicateParseError,
        libpg_query::{self, Token, TokenKind},
        parser::ParsedPredicate,
        placeholders::Placeholder,
    },
    placeholder::parse_body,
};

pub(in crate::predicate) struct TokenizedPredicate {
    pub predicate: ParsedPredicate,
    pub tokens: Vec<Token>,
}

pub(in crate::predicate) fn parse(input: &str) -> Result<TokenizedPredicate, PredicateParseError> {
    if let Some(offset) = input.find('\0') {
        return Err(PredicateParseError {
            message: "SQL condition contains a NUL character".into(),
            source_span: Some(offset..offset + 1),
        });
    }
    let mut tokens = scan_template(input)?.into_iter();
    let mut expression_tokens = Vec::with_capacity(tokens.len());
    let mut placeholders = Vec::new();
    let mut sql = String::with_capacity(input.len());
    let mut copied_end = 0;

    while let Some(token) = tokens.next() {
        let rejection = match token.kind {
            TokenKind::Symbol(';') => Some("Statement separator is not allowed"),
            TokenKind::Parameter(_) | TokenKind::Symbol('$') => {
                Some("Use a named template parameter")
            }
            _ => None,
        };
        if let Some(message) = rejection {
            return Err(PredicateParseError {
                message: message.into(),
                source_span: Some(
                    if matches!(token.kind, TokenKind::Parameter(_) | TokenKind::Symbol('$')) {
                        let start = token.source_span.start;
                        let len = input[start + 1..]
                            .chars()
                            .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
                            .map(char::len_utf8)
                            .sum::<usize>();
                        start..start + 1 + len
                    } else {
                        token.source_span
                    },
                ),
            });
        }
        let remaining = tokens.as_slice();
        if token.kind != TokenKind::Symbol('{')
            || !remaining.first().is_some_and(|next| {
                next.kind == TokenKind::Symbol('{')
                    && next.source_span.start == token.source_span.end
            })
        {
            expression_tokens.push(token);
            continue;
        }

        let closing_index = closing_brace_index(remaining).ok_or_else(|| PredicateParseError {
            message: "Invalid placeholder: Expected two closing braces".into(),
            source_span: Some(token.source_span.start..remaining[0].source_span.end),
        })?;
        let source_span = token.source_span.start..remaining[closing_index].source_span.end;
        let (name, declared_type) =
            parse_body(input, &remaining[1..closing_index - 1]).map_err(|error| {
                PredicateParseError {
                    message: format!("Invalid placeholder: {error}"),
                    source_span: Some(source_span.clone()),
                }
            })?;
        let _ = tokens.nth(closing_index);
        let bind_index = placeholders.len() + 1;
        let marker = format!("${bind_index}");
        sql.push_str(&input[copied_end..source_span.start]);
        // Braces separate tokens in the input, but a bind marker may need whitespace.
        if sql.ends_with(is_identifier_part) {
            sql.push(' ');
        }
        let bind_start = sql.len();
        sql.push_str(&marker);
        let bind_span = bind_start..sql.len();
        copied_end = source_span.end;
        if input[copied_end..].starts_with(is_identifier_part) {
            sql.push(' ');
        }
        placeholders.push(Placeholder {
            bind_index,
            name,
            declared_type,
            source_span: source_span.clone(),
            bind_span,
        });
        expression_tokens.push(Token {
            kind: TokenKind::Parameter(marker),
            source_span,
        });
    }
    sql.push_str(&input[copied_end..]);
    Ok(TokenizedPredicate {
        predicate: ParsedPredicate { sql, placeholders },
        tokens: expression_tokens,
    })
}

fn is_identifier_part(ch: char) -> bool {
    ch.is_alphabetic() || ch.is_ascii_digit() || ch == '$' || ch == '_' || !ch.is_ascii()
}

// The slice starts at the second opening brace.
fn closing_brace_index(tokens: &[Token]) -> Option<usize> {
    let close = tokens
        .iter()
        .position(|token| token.kind == TokenKind::Symbol('}'))?;
    let end = close + 1;
    (tokens.get(end)?.kind == TokenKind::Symbol('}')).then_some(end)
}

fn scan_template(input: &str) -> Result<Vec<Token>, PredicateParseError> {
    let mut scanned = Cow::Borrowed(input);
    loop {
        match libpg_query::scan(&scanned) {
            Ok(tokens) => return Ok(tokens),
            Err(error) => {
                // PostgreSQL rejects e.g. 1name lexically, before our template
                // grammar can report the full {{1name}} span. On that error only,
                // scan the prefix to establish that it is inside an unquoted
                // template, then mask the leading digit without moving any byte.
                // The template parser always reads the original, unmasked text.
                if error
                    .message
                    .starts_with("trailing junk after numeric literal")
                    && let Some(offset) = error.byte_offset
                    && scanned
                        .as_bytes()
                        .get(offset)
                        .is_some_and(u8::is_ascii_digit)
                    && let Ok(prefix) = libpg_query::scan(&scanned[..offset])
                    && has_open_placeholder(&prefix)
                {
                    scanned.to_mut().replace_range(offset..offset + 1, "_");
                    continue;
                }
                return Err(lexical_error(input, error));
            }
        }
    }
}

fn lexical_error(input: &str, error: libpg_query::ParseError) -> PredicateParseError {
    let mut offset = error.byte_offset;
    // Retain the existing API's EOF ranges for incomplete dollar strings,
    // Unicode strings and block comments. Ordinary quoted strings and quoted
    // identifiers keep pointing to their opening delimiter.
    if error
        .message
        .starts_with("unterminated dollar-quoted string")
        || error.message.starts_with("unterminated /* comment")
        || (error.message.starts_with("unterminated quoted string")
            && offset.is_some_and(|offset| {
                input[offset..].starts_with("U&'") || input[offset..].starts_with("u&'")
            }))
    {
        offset = Some(input.len());
    } else if error.message.starts_with("unterminated bit string")
        || error.message.starts_with("unterminated hexadecimal string")
    {
        offset = offset.map(|offset| offset + 1);
    }
    let message = if error.message.starts_with("unterminated quoted string") {
        format!("Unterminated string literal: {}", error.message)
    } else {
        error.message
    };
    PredicateParseError {
        message,
        source_span: offset.map(|offset| offset..offset),
    }
}

fn has_open_placeholder(tokens: &[Token]) -> bool {
    let mut open = false;
    for pair in tokens.windows(2) {
        if pair[0].source_span.end != pair[1].source_span.start {
            continue;
        }
        if pair
            .iter()
            .all(|token| token.kind == TokenKind::Symbol('{'))
        {
            open = true;
        } else if pair
            .iter()
            .all(|token| token.kind == TokenKind::Symbol('}'))
        {
            open = false;
        }
    }
    open
}

#[cfg(test)]
mod tests {
    use super::{TokenKind, TokenizedPredicate, parse};

    #[test]
    fn substitutes_placeholders_before_expression_validation() {
        let parsed = parse("x = {{id}} AND").unwrap().predicate;
        assert_eq!(parsed.sql, "x = $1 AND");
    }

    #[test]
    fn numbered_tokens_keep_the_original_placeholder_ranges() {
        let input = "x = {{id:bigint}} OR (y = {{id}} AND z = {{other}})";
        let TokenizedPredicate { predicate, tokens } = parse(input).unwrap();
        let bind_tokens: Vec<_> = tokens
            .iter()
            .filter_map(|token| match &token.kind {
                TokenKind::Parameter(marker) => {
                    Some((marker.as_str(), &input[token.source_span.clone()]))
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
