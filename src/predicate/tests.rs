use super::parser::{ParameterType, parse_predicate};

#[test]
fn matches_placeholders_and_preserves_sql_outside_their_byte_ranges() {
    let input = r#"note <> 'é ''{{inside}}'' $1 ;'
        AND note <> E'backslash\\quote\' {{escaped}}'
        AND note <> 'plain backslash\'
        AND note <> $tag$${{dollar}}$tag$
        AND "{{quoted""identifier}}" IS NOT NULL
        /* outer /* {{nested}} */ {{comment}} */
        AND account_id = ANY({{accounts:bigint[]}}) -- {{line}} $2
        AND created_at < {{cutoff:date}} AND note = {{label}}
        AND note <> {{label}} -- final comment"#;
    let parsed = parse_predicate(input).unwrap();
    let markers: Vec<_> = parsed
        .placeholders
        .iter()
        .map(|slot| {
            (
                &input[slot.source_span.clone()],
                &parsed.sql[slot.bind_span.clone()],
                slot.name.as_str(),
                slot.declared_type,
            )
        })
        .collect();
    assert_eq!(
        markers,
        [
            (
                "{{accounts:bigint[]}}",
                "$1",
                "accounts",
                Some(ParameterType::BigintArray)
            ),
            ("{{cutoff:date}}", "$2", "cutoff", Some(ParameterType::Date)),
            ("{{label}}", "$3", "label", None),
            ("{{label}}", "$4", "label", None),
        ]
    );
    let expected_sql = input
        .replace("{{accounts:bigint[]}}", "$1")
        .replace("{{cutoff:date}}", "$2")
        .replacen("{{label}}", "$3", 1)
        .replacen("{{label}}", "$4", 1);
    assert_eq!(parsed.sql, expected_sql);
}

#[test]
fn separates_bind_markers_from_adjacent_keywords() {
    let input = "NOT{{active}}AND age>={{minimum}}";
    let parsed = parse_predicate(input).unwrap();
    assert_eq!(parsed.sql, "NOT $1 AND age>=$2");
    assert_eq!(parsed.placeholders[0].bind_span, 4..6);
    assert_eq!(&parsed.sql[parsed.placeholders[1].bind_span.clone()], "$2");
}

#[test]
fn recognizes_defined_type_annotations_without_values() {
    let input = r#"unresolved = {{_text1:text}} AND x = {{n:integer}}
        AND y > {{id:bigint}}::bigint AND {{active:boolean}}
        AND z = ANY({{ids:integer[]}}) AND ts < {{cutoff:date}}"#;
    let parsed = parse_predicate(input).unwrap();
    assert_eq!(
        parsed
            .placeholders
            .iter()
            .map(|slot| slot.declared_type)
            .collect::<Vec<_>>(),
        [
            Some(ParameterType::Text),
            Some(ParameterType::Integer),
            Some(ParameterType::Bigint),
            Some(ParameterType::Boolean),
            Some(ParameterType::IntegerArray),
            Some(ParameterType::Date),
        ]
    );
}

#[test]
fn finds_placeholders_after_unicode_on_the_same_line_and_after_crlf() {
    let input = "note = '🌿' AND x = {{first}}\r\nAND y = {{second}}";
    let parsed = parse_predicate(input).unwrap();
    assert_eq!(parsed.sql, "note = '🌿' AND x = $1\r\nAND y = $2");
    assert_eq!(
        parsed.placeholders[0].source_span.start,
        input.find("{{first}}").unwrap()
    );
    assert_eq!(parsed.placeholders[1].source_span.end, input.len());
    assert_eq!(parsed.placeholders[1].bind_span.end, parsed.sql.len());
}

#[test]
fn rejects_malformed_placeholders_at_their_original_location() {
    for marker in [
        "{{}}",
        "{{1name}}",
        r#"{{"name"}}"#,
        "{{réf}}",
        "{{ value}}",
        "{{value } }",
        "{{value/* comment */}}",
        "{{value\n}}",
        "{{value",
        "{{value:}}",
        "{{value:date [ ]}}",
        "{{value:bigint[][]}}",
        "{{value:uuid}}",
    ] {
        let input = format!("TRUE AND\n  x = {marker}");
        let error = parse_predicate(&input).unwrap_err();
        let start = input.find("{{").unwrap();
        let len = if marker.ends_with("}}") {
            marker.len()
        } else {
            2
        };
        assert_eq!(
            error.source_span,
            Some(start..start + len),
            "{input}: {error}"
        );
        assert!(error.message.starts_with("Invalid placeholder"));
    }
}

#[test]
fn requires_one_complete_expression_and_rejects_raw_bind_markers() {
    for input in [
        "",
        " -- no condition",
        "/* no condition */",
        "id =",
        "id = {{id}} AND",
        "id = {{id}}}",
        "id = {{id}} LIMIT 1",
        "id = {{id}};",
        "id = {{id}}; SELECT 1",
        "EXISTS (SELECT 1; SELECT 2)",
        "id = $1",
        "id = {{id}} OR id = $2",
        "note = 'unclosed",
        "note = '\0'",
    ] {
        assert!(parse_predicate(input).is_err(), "{input:?}");
    }
    let input = "payload ? 'key' AND note = '$1; {{literal}}' -- $2";
    let parsed = parse_predicate(input).unwrap();
    assert!(parsed.placeholders.is_empty());
    assert_eq!(parsed.sql, input);
}

#[test]
fn preserves_both_ranges_for_repeated_parameters_in_nested_queries() {
    let input = r#"note = '🌿' AND id = {{key:bigint}}
        AND EXISTS (
            SELECT 1 FROM sales.orders AS o WHERE o."étiquette" = {{key}}
            AND EXISTS (
                SELECT 1 FROM sales.lines AS l WHERE l.order_id = o.id
                AND l.created_at < {{cutoff:date}}
            )
        )"#;
    let parsed = parse_predicate(input).unwrap();
    let actual: Vec<_> = parsed
        .placeholders
        .iter()
        .map(|slot| {
            (
                slot.bind_index,
                slot.name.as_str(),
                &input[slot.source_span.clone()],
                &parsed.sql[slot.bind_span.clone()],
            )
        })
        .collect();
    assert_eq!(
        actual,
        [
            (1, "key", "{{key:bigint}}", "$1"),
            (2, "key", "{{key}}", "$2"),
            (3, "cutoff", "{{cutoff:date}}", "$3"),
        ]
    );
}

#[test]
fn reports_original_byte_ranges_for_lexical_and_sql_syntax_errors() {
    let lexical = "note = '🌿' AND id = {{id}}\nAND note = 'unclosed";
    let error = parse_predicate(lexical).unwrap_err();
    let opening_quote = lexical.rfind('\'').unwrap();
    assert_eq!(error.source_span, Some(opening_quote..opening_quote));
    assert!(error.message.contains("Unterminated string literal"));

    let syntax = "note = '🌿' AND id = {{identifier:integer}} AND )";
    let wrong_token = syntax.find(')').unwrap();
    let error = parse_predicate(syntax).unwrap_err();
    assert_eq!(error.source_span, Some(wrong_token..wrong_token + 1));
    assert!(error.message.contains("found: )"));
    assert_eq!(error.to_string(), error.message);

    let missing_token = "id = {{id}} AND\r\n";
    let error = parse_predicate(missing_token).unwrap_err();
    assert_eq!(
        error.source_span,
        Some(missing_token.len()..missing_token.len())
    );
}

#[test]
fn highlights_the_original_placeholder_when_syntax_rejects_a_bind_marker() {
    let input = "EXISTS (SELECT 1 FROM {{table}})";
    let error = parse_predicate(input).unwrap_err();
    assert_eq!(&input[error.source_span.unwrap()], "{{table}}");
}

#[test]
fn highlights_the_whole_unexpected_token_after_the_condition() {
    let input = "note = '🌿' AND id = {{id}} LIMIT 1";
    let error = parse_predicate(input).unwrap_err();
    assert_eq!(&input[error.source_span.unwrap()], "LIMIT");
}

#[test]
fn preserves_error_ranges_when_bind_markers_need_whitespace() {
    let input = "note = '🌿' AND NOT{{enabled}}AND x = {{id}}OR )";
    let error = parse_predicate(input).unwrap_err();
    let closing = input.find(')').unwrap();
    assert_eq!(error.source_span, Some(closing..closing + 1));
}

#[test]
fn preserves_case_sensitive_names_and_types_from_original_text() {
    let parsed = parse_predicate("x = {{MyKey:bigint}} OR y = {{select:text}}").unwrap();
    assert_eq!(parsed.sql, "x = $1 OR y = $2");
    assert_eq!(parsed.placeholders[0].name, "MyKey");
    assert_eq!(parsed.placeholders[1].name, "select");
    let input = "x = {{MyKey:BIGINT}}";
    let error = parse_predicate(input).unwrap_err();
    assert_eq!(&input[error.source_span.unwrap()], "{{MyKey:BIGINT}}");
    assert!(error.message.starts_with("Invalid placeholder"));
}

#[test]
fn malformed_numeric_names_keep_the_whole_original_placeholder_range() {
    for marker in [
        "{{1name}}",
        "{{2name}}",
        "{{1e+}}",
        "{{1réf}}",
        "{{key:1type}}",
    ] {
        let input = format!("note = '🌿' AND x = {{{{valid}}}}\r\nAND y = {marker}");
        let error = parse_predicate(&input).unwrap_err();
        assert_eq!(&input[error.source_span.unwrap()], marker);
        assert!(error.message.starts_with("Invalid placeholder"));
    }
    // Braces in a literal or comment must not make a later SQL lexical error
    // look like a malformed template.
    for input in [
        "x = '{{' AND y = 1name",
        "TRUE /* {{ */ AND y = 1name",
        "x = {{valid}} AND y = 1name",
    ] {
        let error = parse_predicate(input).unwrap_err();
        let offset = input.find("1name").unwrap();
        assert_eq!(error.source_span, Some(offset..offset));
        assert!(!error.message.starts_with("Invalid placeholder"));
    }
}

#[test]
fn preserves_raw_marker_error_ranges_outside_strings_and_comments() {
    for marker in ["$1", "$123", "$name", "$1foo", "$réf", "$"] {
        let input = format!("note = '🌿' AND id = {marker}");
        let error = parse_predicate(&input).unwrap_err();
        assert_eq!(&input[error.source_span.unwrap()], marker);
        assert_eq!(error.message, "Use a named template parameter");
    }
}

#[test]
fn preserves_lexical_error_points_for_unterminated_literals_and_comments() {
    for suffix in ["$tag$unclosed", "$$unclosed", "/* unclosed", "U&'unclosed"] {
        let input = format!("note = '🌿'\r\nAND x = {suffix}");
        let error = parse_predicate(&input).unwrap_err();
        assert_eq!(error.source_span, Some(input.len()..input.len()), "{input}");
    }
    for suffix in ["'unclosed", "E'unclosed", "\"unclosed"] {
        let input = format!("note = '🌿'\r\nAND x = {suffix}");
        let error = parse_predicate(&input).unwrap_err();
        let start = input.find(suffix).unwrap();
        assert_eq!(error.source_span, Some(start..start), "{input}");
    }
}
