use std::{
    ffi::{CStr, CString},
    ops::Range,
};

use super::{
    error::{ParseError, copy_error},
    ffi,
};

#[derive(Debug, PartialEq, Eq)]
pub(in crate::predicate) enum TokenKind {
    Word,
    QuotedIdentifier,
    Trivia,
    Symbol(char),
    Parameter(String),
    Other,
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::predicate) struct Token {
    pub kind: TokenKind,
    pub source_span: Range<usize>,
}

/// Return PostgreSQL tokens and original byte ranges, including whitespace and
/// comments as trivia. All native allocations are released before returning.
pub(in crate::predicate) fn scan(input: &str) -> Result<Vec<Token>, ParseError> {
    let text = CString::new(input).map_err(|error| ParseError {
        message: "SQL condition contains a NUL character".into(),
        byte_offset: Some(error.nul_position()),
    })?;
    let result = unsafe { ffi::pg_query_scan_tokens(text.as_ptr()) };
    let tokens = if result.error.is_null() {
        decode_tokens(input, &result)
    } else {
        Err(unsafe { copy_error(input, &*result.error) })
    };
    unsafe { ffi::pg_query_free_scan_tokens_result(result) };
    tokens
}

fn decode_tokens(input: &str, result: &ffi::ScanResult) -> Result<Vec<Token>, ParseError> {
    let invalid_range = || ParseError {
        message: "PostgreSQL scanner returned an invalid token range".into(),
        byte_offset: None,
    };
    let count = usize::try_from(result.n_tokens).map_err(|_| invalid_range())?;
    let native_tokens = if count == 0 {
        &[]
    } else {
        if result.tokens.is_null() {
            return Err(invalid_range());
        }
        // The native result owns exactly n_tokens initialized entries until freed.
        unsafe { std::slice::from_raw_parts(result.tokens, count) }
    };
    let mut tokens = Vec::with_capacity(count);
    let mut end = 0;
    for native in native_tokens {
        let start = usize::try_from(native.start).map_err(|_| invalid_range())?;
        let next_end = usize::try_from(native.end).map_err(|_| invalid_range())?;
        let raw = input
            .get(start..next_end)
            .filter(|_| start >= end && start < next_end)
            .ok_or_else(invalid_range)?;
        if start > end {
            tokens.push(Token {
                kind: TokenKind::Trivia,
                source_span: end..start,
            });
        }
        tokens.push(Token {
            kind: token_kind(native, raw),
            source_span: start..next_end,
        });
        end = next_end;
    }
    if end < input.len() {
        tokens.push(Token {
            kind: TokenKind::Trivia,
            source_span: end..input.len(),
        });
    }
    Ok(tokens)
}

fn token_kind(token: &ffi::ScanToken, raw: &str) -> TokenKind {
    // Single-character tokens use ASCII codes, including braces, which do not
    // have names in the generated token enum. Named token codes may change
    // between PostgreSQL releases, so classify those by their exported names.
    if (1..128).contains(&token.token) {
        return TokenKind::Symbol(char::from(token.token as u8));
    }
    let name = unsafe { ffi::pg_query_token_name(token.token) };
    let name = if name.is_null() {
        &[][..]
    } else {
        unsafe { CStr::from_ptr(name) }.to_bytes()
    };
    match name {
        b"SQL_COMMENT" | b"C_COMMENT" => TokenKind::Trivia,
        b"UIDENT" => TokenKind::QuotedIdentifier,
        b"IDENT" if raw.starts_with('"') => TokenKind::QuotedIdentifier,
        b"IDENT" => TokenKind::Word,
        b"PARAM" => TokenKind::Parameter(raw.into()),
        _ if token.keyword_kind != 0 => TokenKind::Word,
        _ => TokenKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::{TokenKind, scan};

    #[test]
    fn preserves_case_and_byte_ranges_and_keeps_literals_and_comments_opaque() {
        let input =
            "Name = '🌿 {{literal}}'\r\n/* nested /* {{hidden}} */ */ AND \"{{quoted}}\" = {{Key}}";
        let tokens = scan(input).unwrap();
        let words: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Word)
            .map(|t| &input[t.source_span.clone()])
            .collect();
        assert_eq!(words, ["Name", "AND", "Key"]);
        assert_eq!(
            tokens
                .iter()
                .filter(|t| t.kind == TokenKind::Symbol('{'))
                .count(),
            2
        );
        assert_eq!(
            tokens
                .iter()
                .map(|t| &input[t.source_span.clone()])
                .collect::<String>(),
            input
        );
    }

    #[test]
    fn native_errors_use_original_utf8_byte_positions() {
        let input = "note = '🌿'\r\nAND note = 'unclosed";
        assert_eq!(scan(input).unwrap_err().byte_offset, input.rfind('\''));
    }

    #[test]
    fn handles_empty_input_and_retains_only_trivia_input() {
        assert!(scan("").unwrap().is_empty());
        let input = " \r\n/* comment */\t-- final";
        let tokens = scan(input).unwrap();
        assert!(tokens.iter().all(|t| t.kind == TokenKind::Trivia));
        assert_eq!(
            tokens
                .iter()
                .map(|t| &input[t.source_span.clone()])
                .collect::<String>(),
            input
        );
    }
}
