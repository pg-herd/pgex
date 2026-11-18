use std::path::Path;

use super::yaml::{load, parse};
use crate::{
    config::{
        compiler::compile_config,
        input::{CliInput, PredicateInput, PredicateOrigin},
        snapshot_mode::SnapshotMode,
    },
    predicate::{parameters::validation::ParameterOrigin, parser::parse_predicate},
};

#[tokio::test]
async fn loads_the_agreed_yaml_shape_and_preserves_explicit_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pgex.yml");
    tokio::fs::write(
        &path,
        r#"
source:
  connection:
    url: "postgresql://source/app"
    user: sync_user
    ca_file: ca.pem
  session:
    row_security_off: false
  snapshot:
    mode: per-statement
    reader_connection_limit: 3
    exporter_timeouts:
      transaction_ms: 0
    reader_timeouts:
      idle_in_transaction_ms: 250
  selection:
    tables: [public.orders]
    where: >
      account_id = {{id:bigint}}
      AND active
    table_rules:
      - has_column: tenant_id
        where: "tenant_id = {{id:bigint}}"
    table_filters:
      public.orders:
        where: "external_id = {{id:text}}"
target:
  connection:
    url: "postgres://target/app"
  session:
    replica_role: true
  transfer:
    workers: 4
    validate_layouts: false
    no_locks: true
    truncate: true
  delete_filters:
    public.orders: "billing_id = {{id:bigint}}"
params:
  id: "42"
"#,
    )
    .await
    .unwrap();

    let settings = load(&path).await.unwrap().settings;
    assert_eq!(
        settings.source.connection.user.as_deref(),
        Some("sync_user")
    );
    assert_eq!(
        settings.source.connection.ca_file.as_deref(),
        Some(Path::new("ca.pem"))
    );
    assert_eq!(settings.source.session.row_security_off, Some(false));
    assert_eq!(
        settings.source.snapshot.mode,
        Some(SnapshotMode::PerStatement)
    );
    assert_eq!(
        settings
            .source
            .snapshot
            .reader_connection_limit
            .unwrap()
            .get(),
        3
    );
    assert_eq!(
        settings.source.snapshot.exporter_timeouts.transaction_ms,
        Some(0)
    );
    assert_eq!(
        settings
            .source
            .snapshot
            .reader_timeouts
            .idle_in_transaction_ms,
        Some(250)
    );
    assert_eq!(
        settings.source.snapshot.client_connection_check_interval_ms,
        None
    );
    assert_eq!(
        settings.source.selection.tables.unwrap(),
        ["public.orders".parse().unwrap()]
    );

    let shared = settings.source.selection.shared.unwrap();
    assert_eq!(shared.original, "account_id = {{id:bigint}} AND active\n");
    assert_eq!(shared.parsed.sql, "account_id = $1 AND active\n");
    assert!(
        matches!(shared.origin, PredicateOrigin::Yaml { file, field }
        if file == path && field == "source.selection.where")
    );
    assert_eq!(
        settings.source.selection.table_rules.unwrap()[0].has_column,
        "tenant_id"
    );
    assert_eq!(
        settings.source.selection.table_filters.unwrap()[0]
            .condition
            .parsed
            .sql,
        "external_id = $1"
    );
    assert_eq!(settings.target.session.replica_role, Some(true));
    assert_eq!(settings.target.transfer.workers.unwrap().get(), 4);
    assert_eq!(settings.target.transfer.validate_layouts, Some(false));
    assert_eq!(settings.target.transfer.no_locks, Some(true));
    assert_eq!(settings.target.transfer.truncate, Some(true));
    assert_eq!(
        settings.target.delete_filters.unwrap()[0]
            .condition
            .parsed
            .sql,
        "billing_id = $1"
    );
    assert_eq!(settings.params[0].raw_value, "42");
    assert_eq!(
        settings.params[0].origin,
        ParameterOrigin::Yaml { file: path }
    );
}

#[test]
fn gives_the_yaml_field_and_sql_location_for_each_predicate_kind() {
    for (yaml, field) in [
        (
            "source: {selection: {where: 'id = )'}}",
            "source.selection.where",
        ),
        (
            "source: {selection: {table_rules: [{has_column: id, where: 'id = )'}]}}",
            "source.selection.table_rules[0].where",
        ),
        (
            "source: {selection: {table_filters: {public.orders: {where: 'id = )'}}}}",
            "source.selection.table_filters.public.orders.where",
        ),
        (
            "target: {delete_filters: {public.orders: 'id = )'}}",
            "target.delete_filters.public.orders",
        ),
    ] {
        let error = parse_error(yaml);
        assert!(error.contains("pgex.yml"), "{error}");
        assert!(error.contains(field), "{error}");
        assert!(error.contains("Line 1, column 6"), "{error}");
        assert!(error.contains("1 | id = )"), "{error}");
    }
}

#[test]
fn rejects_a_malformed_yaml_predicate_even_with_a_valid_cli_replacement() {
    let mut cli = CliInput::default();
    cli.settings.source.connection.url = Some("postgresql://source/app".into());
    cli.settings.target.connection.url = Some("postgresql://target/app".into());
    cli.settings.source.selection.shared = Some(PredicateInput {
        original: "active".into(),
        parsed: parse_predicate("active").unwrap(),
        origin: PredicateOrigin::Cli {
            flag: "--where".into(),
        },
    });

    let result = parse(
        "source: {selection: {where: 'id = {{broken'}}",
        Path::new("pgex.yml"),
    )
    .and_then(|file| compile_config(cli, Some(file)));
    let error = format!(
        "{:#}",
        result.err().expect("Expected YAML predicate syntax error")
    );
    assert!(error.contains("source.selection.where"), "{error}");
    assert!(error.contains("Expected two closing braces"), "{error}");
}

#[test]
fn defers_missing_and_invalid_parameter_values_to_the_compiler() {
    for (yaml_params, expected) in [
        ("", "Missing parameter 'id'"),
        ("params: {id: 'invalid'}", "must be a valid bigint"),
    ] {
        let yaml =
            format!("source: {{selection: {{where: 'id = {{{{id:bigint}}}}'}}}}\n{yaml_params}");
        let file = parse(&yaml, Path::new("pgex.yml")).unwrap();
        let mut cli = CliInput::default();
        cli.settings.source.connection.url = Some("postgresql://source/app".into());
        cli.settings.target.connection.url = Some("postgresql://target/app".into());
        let error = compile_config(cli, Some(file))
            .err()
            .expect("Expected a parameter error");
        assert!(error.to_string().contains(expected), "{error:#}");
    }
}

#[test]
fn rejects_unknown_fields_duplicate_keys_and_non_string_predicates() {
    for (yaml, expected) in [
        ("target: {transfer: {workerz: 4}}", "workerz"),
        ("params: {id: '1', id: '2'}", "duplicate"),
        (
            "target: {delete_filters: {public.orders: true}}",
            "a YAML string",
        ),
        ("params: {id: 42}", "a YAML string"),
    ] {
        let error = parse_error(yaml);
        assert!(
            error.to_lowercase().contains(&expected.to_lowercase()),
            "{error}"
        );
    }
}

#[test]
fn rejects_invalid_file_settings_with_their_paths() {
    let error =
        parse_error("source: {connection: {url: 'host=example dbname=app password=private'}}");
    assert!(error.contains("source.connection.url"), "{error}");
    assert!(!error.contains("private"), "{error}");

    let error = parse_error("source: {selection: {tables: [orders]}}");
    assert!(error.contains("source.selection.tables[0]"), "{error}");

    let error = parse_error("source: {snapshot: {reader_timeouts: {transaction_ms: 2147483648}}}");
    assert!(
        error.contains("source.snapshot.reader_timeouts.transaction_ms"),
        "{error}"
    );
}

fn parse_error(yaml: &str) -> String {
    let error = parse(yaml, Path::new("pgex.yml"))
        .err()
        .expect("Expected an invalid file");
    error.to_string()
}
