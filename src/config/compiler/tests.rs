use std::{num::NonZeroUsize, path::PathBuf};

use super::compile_config;
use crate::{
    config::{
        compiled::{SourceFilterPolicy, TableSelection},
        input::{
            CliInput, ColumnRuleInput, ConfigInput, FileConfig, PredicateInput, PredicateOrigin,
            TableFilterInput,
        },
        snapshot_mode::SnapshotMode,
    },
    predicate::{
        parameters::{
            parsing::ParameterValue,
            validation::{ParameterInput, ParameterOrigin},
        },
        parser::parse_predicate,
    },
};

#[test]
fn applies_explicit_values_including_false_and_zero_then_fills_defaults() {
    let mut file = settings();
    file.source.connection.user = Some("file_user".into());
    file.source.connection.password = Some("file_password".into());
    file.source.connection.ca_file = Some("ca.pem".into());
    file.source.session.row_security_off = Some(true);
    file.source.snapshot.reader_timeouts.transaction_ms = Some(500);
    file.source.snapshot.reader_timeouts.idle_in_transaction_ms = Some(300);
    file.source.snapshot.reader_connection_limit = NonZeroUsize::new(2);
    file.target.session.row_security_off = Some(true);
    file.target.session.replica_role = Some(true);
    file.target.transfer.skip_delete = Some(true);
    file.target.transfer.no_locks = Some(true);

    let mut cli = ConfigInput::default();
    cli.source.connection.url = Some("postgresql://url_user@cli/app".into());
    cli.source.connection.password = Some("cli_password".into());
    cli.source.session.row_security_off = Some(false);
    cli.source.snapshot.reader_timeouts.transaction_ms = Some(0);
    cli.target.session.replica_role = Some(false);
    cli.target.transfer.skip_delete = Some(false);
    cli.target.transfer.no_locks = Some(false);
    cli.target.transfer.workers = NonZeroUsize::new(4);

    let compiled = compile_config(
        CliInput { settings: cli },
        Some(FileConfig { settings: file }),
    )
    .unwrap();
    assert_eq!(
        compiled.source.connection.url,
        "postgresql://url_user@cli/app"
    );
    assert_eq!(
        compiled.source.connection.user.as_deref(),
        Some("file_user")
    );
    assert_eq!(
        compiled.source.connection.password.as_deref(),
        Some("cli_password")
    );
    assert_eq!(
        compiled.source.connection.ca_file,
        Some(PathBuf::from("ca.pem"))
    );
    assert_eq!(compiled.target.connection.url, "postgresql://target/app");
    assert!(!compiled.source.session.row_security_off);
    assert_eq!(compiled.source.snapshot.mode, SnapshotMode::Shared);
    assert_eq!(compiled.source.snapshot.reader_connection_limit.get(), 2);
    assert_eq!(
        compiled.source.snapshot.client_connection_check_interval_ms,
        1_000
    );
    assert_eq!(compiled.source.snapshot.reader_timeouts.transaction_ms, 0);
    assert_eq!(
        compiled
            .source
            .snapshot
            .reader_timeouts
            .idle_in_transaction_ms,
        300
    );
    assert!(compiled.target.session.row_security_off);
    assert!(!compiled.target.session.replica_role);
    assert!(!compiled.target.transfer.skip_delete);
    assert!(!compiled.target.transfer.no_locks);
    assert_eq!(compiled.target.transfer.workers.get(), 4);
    assert!(compiled.target.transfer.validate_layouts);
    assert_eq!(compiled.target.transfer.commit_group_mib.get(), 32);
    assert!(matches!(
        compiled.source.selection.tables,
        TableSelection::Discover
    ));
}

#[test]
fn explicit_cli_snapshot_setting_takes_precedence_over_yaml() {
    use SnapshotMode::{PerStatement, Shared};

    for (cli_mode, file_mode, expected) in [
        (None, Some(PerStatement), PerStatement),
        (Some(Shared), Some(PerStatement), Shared),
        (Some(PerStatement), Some(Shared), PerStatement),
    ] {
        let mut cli = CliInput::default();
        cli.settings.source.snapshot.mode = cli_mode;
        let mut file = settings();
        file.source.snapshot.mode = file_mode;

        let compiled = compile_config(cli, Some(FileConfig { settings: file })).unwrap();
        assert_eq!(compiled.source.snapshot.mode, expected);
    }
}

#[test]
fn compiles_mixed_occurrence_types_using_raw_values_merged_by_name() {
    let mut file = settings();
    file.params = vec![
        file_param("id", "old_yaml_value"),
        file_param("label", "yaml label"),
    ];
    file.source.selection.shared = Some(file_predicate(
        "id = {{discarded}}",
        "source.selection.where",
    ));
    file.source.selection.table_rules = Some(vec![ColumnRuleInput {
        has_column: "account_id".into(),
        condition: file_predicate(
            "account_id = {{id:integer}}",
            "source.selection.table_rules[0].where",
        ),
    }]);
    file.target.delete_filters = Some(vec![TableFilterInput {
        table: "public.orders".parse().unwrap(),
        condition: file_predicate(
            "external_id = {{id:text}}",
            "target.delete_filters.public.orders",
        ),
    }]);

    let mut cli = ConfigInput::default();
    cli.params.push(cli_param("id", "42"));
    cli.source.selection.shared = Some(cli_predicate(
        "id = {{id:bigint}} AND external_id = {{id:text}} AND alias = {{id}} AND label = {{label}}",
    ));
    let compiled = compile_config(
        CliInput { settings: cli },
        Some(FileConfig { settings: file }),
    )
    .unwrap();
    let SourceFilterPolicy {
        shared: Some(shared),
        table_rules,
        ..
    } = compiled.source.selection.filters
    else {
        panic!("Expected compiled source predicates");
    };
    assert_eq!(
        shared.sql,
        "id = $1 AND external_id = $2 AND alias = $3 AND label = $4"
    );
    let slots: Vec<_> = shared
        .parameters
        .iter()
        .map(|slot| (&shared.sql[slot.bind_span.clone()], &slot.value))
        .collect();
    assert_eq!(
        slots,
        [
            ("$1", &ParameterValue::Bigint(42)),
            ("$2", &ParameterValue::Text("42".into())),
            ("$3", &ParameterValue::Text("42".into())),
            ("$4", &ParameterValue::Text("yaml label".into())),
        ]
    );
    assert_eq!(
        table_rules[0].condition.parameters[0].value,
        ParameterValue::Integer(42)
    );
    assert_eq!(
        compiled.target.delete_filters[0].condition.parameters[0].value,
        ParameterValue::Text("42".into())
    );
}

#[test]
fn invalid_value_points_to_the_occurrence_that_rejects_it_and_omits_the_value() {
    let mut file = settings();
    let sql = "external_id = {{id:text}}\nAND id = {{id:bigint}}";
    file.source.selection.shared = Some(file_predicate(sql, "source.selection.where"));
    let mut cli = ConfigInput::default();
    cli.params.push(cli_param("id", "private-invalid-value"));

    let error = compile_config(
        CliInput { settings: cli },
        Some(FileConfig { settings: file }),
    )
    .err()
    .expect("Expected an invalid value error")
    .to_string();
    assert_eq!(
        error,
        concat!(
            "pgex.yml (source.selection.where)\n",
            "Parameter 'id' from --param must be a valid bigint\n",
            "Line 2, column 10\n\n",
            "2 | AND id = {{id:bigint}}\n",
            "             ^^^^^^^^^^^^^",
        )
    );
}

#[test]
fn rejects_missing_unused_and_duplicate_parameters() {
    let mut missing = settings();
    missing.source.selection.shared = Some(cli_predicate("id = {{id}}"));
    assert!(config_error(missing).contains("--where\nMissing parameter 'id'"));

    let mut unused = settings();
    unused.params.push(cli_param("unused", "42"));
    assert_eq!(
        config_error(unused),
        "Unused parameter 'unused' from --param"
    );

    let mut duplicate = settings();
    duplicate.params = vec![cli_param("id", "42"), cli_param("id", "43")];
    assert_eq!(
        config_error(duplicate),
        "Duplicate parameter 'id' from --param"
    );
}

#[test]
fn cli_table_list_replaces_the_file_list_and_filters_must_match_it() {
    let mut file = settings();
    file.source.selection.tables = Some(vec!["public.invoices".parse().unwrap()]);
    let mut cli = ConfigInput::default();
    cli.source.selection.tables = Some(vec!["public.orders".parse().unwrap()]);

    let compiled = compile_config(
        CliInput { settings: cli },
        Some(FileConfig { settings: file }),
    )
    .unwrap();
    let TableSelection::Explicit(tables) = compiled.source.selection.tables else {
        panic!("Expected an explicit table list");
    };
    assert_eq!(tables, vec!["public.orders".parse().unwrap()]);

    let mut input = settings();
    input.source.selection.tables = Some(tables);
    input.target.delete_filters = Some(vec![TableFilterInput {
        table: "public.invoices".parse().unwrap(),
        condition: file_predicate("TRUE", "target.delete_filters.public.invoices"),
    }]);
    assert!(
        config_error(input).contains("in target.delete_filters is outside source.selection.tables")
    );
}

#[test]
fn omitted_where_leaves_the_source_filter_policy_empty() {
    let compiled = compile_config(
        CliInput {
            settings: settings(),
        },
        None,
    )
    .unwrap();
    let filters = compiled.source.selection.filters;
    assert!(filters.shared.is_none());
    assert!(filters.table_rules.is_empty());
    assert!(filters.table_filters.is_empty());
    assert!(compiled.target.delete_filters.is_empty());
}

#[test]
fn requires_both_connections_and_rejects_empty_or_duplicate_table_lists() {
    let mut input = settings();
    input.target.connection.url = None;
    assert_eq!(config_error(input), "Missing target.connection.url");

    let mut input = settings();
    input.source.selection.tables = Some(vec![]);
    assert_eq!(
        config_error(input),
        "source.selection.tables must contain at least one table"
    );

    let mut input = settings();
    input.source.selection.tables = Some(vec!["public.orders".parse().unwrap(); 2]);
    assert!(config_error(input).contains("Duplicate table"));
}

fn settings() -> ConfigInput {
    let mut settings = ConfigInput::default();
    settings.source.connection.url = Some("postgresql://source/app".into());
    settings.target.connection.url = Some("postgresql://target/app".into());
    settings
}

fn cli_predicate(sql: &str) -> PredicateInput {
    PredicateInput {
        original: sql.into(),
        parsed: parse_predicate(sql).unwrap(),
        origin: PredicateOrigin::Cli {
            flag: "--where".into(),
        },
    }
}

fn file_predicate(sql: &str, field: &str) -> PredicateInput {
    PredicateInput {
        original: sql.into(),
        parsed: parse_predicate(sql).unwrap(),
        origin: PredicateOrigin::Yaml {
            file: "pgex.yml".into(),
            field: field.into(),
        },
    }
}

fn cli_param(name: &str, value: &str) -> ParameterInput {
    ParameterInput {
        name: name.into(),
        raw_value: value.into(),
        origin: ParameterOrigin::Cli,
    }
}

fn file_param(name: &str, value: &str) -> ParameterInput {
    ParameterInput {
        name: name.into(),
        raw_value: value.into(),
        origin: ParameterOrigin::Yaml {
            file: "pgex.yml".into(),
        },
    }
}

fn config_error(settings: ConfigInput) -> String {
    compile_config(CliInput { settings }, None)
        .err()
        .expect("Expected a config error")
        .to_string()
}
