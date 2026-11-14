use std::ops::Range;

use sqlparser::tokenizer::{Location, Span};

/// Maps the SQL parser's character positions to UTF-8 byte offsets.
pub(in crate::predicate) struct SourceLocations {
    line_starts: Vec<usize>,
    char_offsets: Vec<usize>,
}

impl SourceLocations {
    pub(in crate::predicate) fn new(input: &str) -> Self {
        let mut line_starts = vec![0];
        let mut char_offsets = Vec::new();
        for (offset, ch) in input.char_indices() {
            char_offsets.push(offset);
            if ch == '\n' {
                line_starts.push(char_offsets.len());
            }
        }
        char_offsets.push(input.len());
        Self {
            line_starts,
            char_offsets,
        }
    }

    fn offset(&self, location: Location) -> Option<usize> {
        let line = usize::try_from(location.line).ok()?;
        let column = usize::try_from(location.column).ok()?;
        let line_start = *self.line_starts.get(line.checked_sub(1)?)?;
        let index = line_start.checked_add(column.checked_sub(1)?)?;
        if self
            .line_starts
            .get(line)
            .is_some_and(|&next| index >= next)
        {
            return None;
        }
        self.char_offsets.get(index).copied()
    }

    pub(in crate::predicate) fn point(&self, location: Location) -> Option<Range<usize>> {
        let offset = self.offset(location)?;
        Some(offset..offset)
    }

    pub(in crate::predicate) fn span(&self, span: Span) -> Option<Range<usize>> {
        Some(self.offset(span.start)?..self.offset(span.end)?)
    }
}
