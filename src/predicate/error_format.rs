use std::{fmt::Write, ops::Range};

use unicode_width::UnicodeWidthStr;

use super::error::PredicateParseError;

const TAB_WIDTH: usize = 4;

/// Formats the message and its location within the original predicate.
pub fn format_predicate_error(input: &str, error: &PredicateParseError) -> String {
    format_predicate_diagnostic(input, &error.message, error.source_span.as_ref())
}

pub(crate) fn format_predicate_diagnostic(
    input: &str,
    message: &str,
    span: Option<&Range<usize>>,
) -> String {
    let mut output = message.to_owned();
    let Some(span) = span else {
        return output;
    };
    let Some(highlighted) = input.get(span.clone()) else {
        return output;
    };

    let prefix = &input[..span.start];
    let first_line = prefix.bytes().filter(|&byte| byte == b'\n').count() + 1;
    let mut line_start = prefix.rfind('\n').map_or(0, |offset| offset + 1);
    let column = input[line_start..span.start].chars().count() + 1;
    let lines = highlighted
        .strip_suffix('\n')
        .unwrap_or(highlighted)
        .bytes()
        .filter(|&byte| byte == b'\n')
        .count()
        + 1;
    let gutter = (first_line + lines - 1).to_string().len();

    write!(output, "\nLine {first_line}, column {column}\n").unwrap();
    for (index, raw_line) in input[line_start..].split('\n').take(lines).enumerate() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let start = span.start.saturating_sub(line_start).min(line.len());
        let end = span.end.saturating_sub(line_start).min(line.len());
        let mut visible = String::new();
        let prefix_width = append_visible(&mut visible, &line[..start], 0);
        let end_column = append_visible(&mut visible, &line[start..end], prefix_width);
        append_visible(&mut visible, &line[end..], end_column);
        let indent = gutter + 3 + prefix_width;
        let underline = "^".repeat((end_column - prefix_width).max(1));
        write!(
            output,
            "\n{:>gutter$} | {visible}\n{:indent$}{underline}",
            first_line + index,
            ""
        )
        .unwrap();
        line_start += raw_line.len() + 1;
    }
    output
}

fn append_visible(output: &mut String, text: &str, mut column: usize) -> usize {
    for segment in text.split_inclusive(char::is_control) {
        let control = segment.chars().next_back().filter(|ch| ch.is_control());
        let plain = match control {
            Some(ch) => &segment[..segment.len() - ch.len_utf8()],
            None => segment,
        };
        output.push_str(plain);
        column += plain.width();
        match control {
            Some('\t') => {
                let spaces = TAB_WIDTH - column % TAB_WIDTH;
                output.extend(std::iter::repeat_n(' ', spaces));
                column += spaces;
            }
            Some(ch) => {
                let escaped = ch.escape_default();
                column += escaped.len();
                output.extend(escaped);
            }
            None => {}
        }
    }
    column
}

#[cfg(test)]
mod tests {
    use super::{PredicateParseError, format_predicate_error};
    use crate::predicate::parser::parse_predicate;

    #[test]
    fn highlights_a_parser_error_in_the_original_predicate() {
        let input = concat!("created_at < {{cutoff:date}}\n", "AND )",);
        let expected = concat!(
            "Expected: an expression, found: )\n",
            "Line 2, column 5\n",
            "\n",
            "2 | AND )\n",
            "        ^",
        );

        let error = parse_predicate(input).unwrap_err();
        assert_eq!(format_predicate_error(input, &error), expected);
    }

    #[test]
    fn underlines_the_whole_range_after_a_wide_unicode_character_and_a_tab() {
        let input = "\tnote = '🌿' AND id = {{id}}";
        let expected = concat!(
            "Invalid parameter\n",
            "Line 1, column 22\n",
            "\n",
            "1 |     note = '🌿' AND id = {{id}}\n",
            "                             ^^^^^^",
        );

        let start = input.find("{{id}}").unwrap();
        let error = PredicateParseError {
            message: "Invalid parameter".into(),
            source_span: Some(start..input.len()),
        };
        assert_eq!(format_predicate_error(input, &error), expected);
    }

    #[test]
    fn highlights_an_exact_range_across_crlf_lines() {
        let input = concat!("x = {{id:\r\n", "  invalid}} AND true",);
        let expected = concat!(
            "Invalid placeholder: Expected an unquoted parameter type\n",
            "Line 1, column 5\n",
            "\n",
            "1 | x = {{id:\n",
            "        ^^^^^\n",
            "2 |   invalid}} AND true\n",
            "    ^^^^^^^^^^^",
        );

        let error = parse_predicate(input).unwrap_err();
        assert_eq!(format_predicate_error(input, &error), expected);
    }

    #[test]
    fn marks_end_of_input_on_an_empty_final_line() {
        let input = "id =\n";
        let expected = concat!(
            "Expected: an expression, found: EOF\n",
            "Line 2, column 1\n",
            "\n",
            "2 | \n",
            "    ^",
        );

        let error = parse_predicate(input).unwrap_err();
        assert_eq!(format_predicate_error(input, &error), expected);
    }

    #[test]
    fn displays_the_message_when_the_range_cannot_be_highlighted() {
        for source_span in [None, Some(1..2), Some(0..10)] {
            let error = PredicateParseError {
                message: "Invalid expression".into(),
                source_span,
            };
            assert_eq!(format_predicate_error("🌿", &error), "Invalid expression");
        }
    }
}
