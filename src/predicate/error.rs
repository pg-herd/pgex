use std::{fmt, ops::Range};

#[derive(Debug)]
pub struct PredicateParseError {
    /// Half-open UTF-8 byte range in the original condition.
    /// An empty range marks a point, such as the end of input.
    pub source_span: Option<Range<usize>>,
    pub message: String,
}

impl fmt::Display for PredicateParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PredicateParseError {}
