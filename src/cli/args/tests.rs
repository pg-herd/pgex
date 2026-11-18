use clap::error::ErrorKind;
use pgex::config::snapshot_mode::SnapshotMode;

use super::{Cli, Parser, VarError, resolve_password};

#[test]
fn omitted_cli_settings_are_left_for_the_compiler() {
    let cli = Cli::try_parse_from(["pgex", "--config", "pgex.yml"])
        .unwrap()
        .into_input()
        .unwrap();
    assert_eq!(
        cli.config_path.as_deref(),
        Some(std::path::Path::new("pgex.yml"))
    );
    let settings = cli.input.settings;
    assert!(settings.source.connection.url.is_none());
    assert!(settings.target.connection.url.is_none());
    assert!(settings.source.connection.user.is_none());
    assert!(settings.target.connection.user.is_none());
    assert!(settings.source.selection.tables.is_none());
    assert!(settings.source.session.row_security_off.is_none());
    assert!(settings.source.snapshot.mode.is_none());
    assert!(settings.source.snapshot.reader_connection_limit.is_none());
    assert!(settings.target.session.row_security_off.is_none());
    assert!(settings.target.session.replica_role.is_none());
    assert!(settings.target.transfer.workers.is_none());
    assert!(settings.target.transfer.validate_layouts.is_none());
    assert!(settings.target.transfer.no_locks.is_none());
    assert!(settings.target.transfer.skip_delete.is_none());
    assert!(settings.target.transfer.truncate.is_none());
}

#[test]
fn passes_each_cli_user_to_its_own_connection() {
    let settings = Cli::try_parse_from([
        "pgex",
        "--source-user",
        "source_login",
        "--target-user",
        "target_login",
    ])
    .unwrap()
    .into_input()
    .unwrap()
    .input
    .settings;

    assert_eq!(
        settings.source.connection.user.as_deref(),
        Some("source_login")
    );
    assert_eq!(
        settings.target.connection.user.as_deref(),
        Some("target_login")
    );
}

#[test]
fn preserves_bare_switches_and_explicit_false() {
    let settings = Cli::try_parse_from([
        "pgex",
        "--source-row-security-off=false",
        "--target-row-security-off",
        "--target-replica-role=false",
        "--target-skip-delete",
        "--target-truncate",
        "--no-target-locks",
        "--target-validate-layouts",
        "false",
    ])
    .unwrap()
    .into_input()
    .unwrap()
    .input
    .settings;

    assert_eq!(settings.source.session.row_security_off, Some(false));
    assert_eq!(settings.target.session.row_security_off, Some(true));
    assert_eq!(settings.target.session.replica_role, Some(false));
    assert_eq!(settings.target.transfer.skip_delete, Some(true));
    assert_eq!(settings.target.transfer.truncate, Some(true));
    assert_eq!(settings.target.transfer.no_locks, Some(true));
    assert_eq!(settings.target.transfer.validate_layouts, Some(false));
}

#[test]
fn accepts_the_two_source_snapshot_modes_and_requires_a_value() {
    for (value, expected) in [
        ("shared", SnapshotMode::Shared),
        ("per-statement", SnapshotMode::PerStatement),
    ] {
        let settings = Cli::try_parse_from(["pgex", "--source-snapshot", value])
            .unwrap()
            .into_input()
            .unwrap()
            .input
            .settings;
        assert_eq!(settings.source.snapshot.mode, Some(expected));
    }

    for args in [
        &["pgex", "--source-snapshot"][..],
        &["pgex", "--source-snapshot=off"],
    ] {
        let Err(error) = Cli::try_parse_from(args) else {
            panic!("Invalid source snapshot mode was accepted");
        };
        assert_eq!(error.kind(), ErrorKind::InvalidValue);
    }
}

#[test]
fn parses_where_and_repeatable_raw_parameters() {
    let original = "id = {{id:bigint}} AND label = {{label}} AND notes = {{notes}}";
    let settings = Cli::try_parse_from([
        "pgex",
        "--where",
        original,
        "--param",
        "id=still raw",
        "--param=label=a=b",
        "--param",
        "notes=",
    ])
    .unwrap()
    .into_input()
    .unwrap()
    .input
    .settings;

    let predicate = settings.source.selection.shared.unwrap();
    assert_eq!(predicate.original, original);
    assert_eq!(
        predicate.parsed.sql,
        "id = $1 AND label = $2 AND notes = $3"
    );
    assert!(matches!(predicate.origin, super::PredicateOrigin::Cli { flag } if flag == "--where"));
    let params: Vec<_> = settings
        .params
        .iter()
        .map(|param| (param.name.as_str(), param.raw_value.as_str()))
        .collect();
    assert_eq!(
        params,
        [("id", "still raw"), ("label", "a=b"), ("notes", "")]
    );
}

#[test]
fn where_syntax_errors_name_the_flag_and_highlight_the_original_clause() {
    let error = Cli::try_parse_from(["pgex", "--where", "id = )"])
        .unwrap()
        .into_input()
        .err()
        .expect("Expected a predicate syntax error");
    assert_eq!(error.kind(), ErrorKind::ValueValidation);
    let message = error.to_string();
    assert!(message.contains("Invalid --where predicate"), "{message}");
    assert!(message.contains("Line 1, column 6"), "{message}");
    assert!(message.contains("1 | id = )"), "{message}");
}

#[test]
fn rejects_malformed_parameter_arguments_without_echoing_supplied_values() {
    for argument in ["private-value", "bad-name=private-value", "=private-value"] {
        let error = Cli::try_parse_from(["pgex", "--param", argument])
            .unwrap()
            .into_input()
            .err()
            .expect("Expected an invalid parameter argument");
        assert_eq!(error.kind(), ErrorKind::ValueValidation);
        let message = error.to_string();
        assert!(message.contains("--param"), "{message}");
        assert!(!message.contains("private-value"), "{message}");
    }
}

#[test]
fn source_reader_limit_rejects_zero() {
    let error = Cli::try_parse_from([
        "pgex",
        "--source",
        "dbname=source",
        "--target",
        "dbname=target",
        "--source-reader-limit",
        "0",
    ])
    .err()
    .unwrap();
    assert_eq!(error.kind(), ErrorKind::ValueValidation);
}

#[test]
fn omitting_tables_preserves_the_absence_of_an_explicit_list() {
    let cli = Cli::try_parse_from([
        "pgex",
        "--source",
        "dbname=app",
        "--target",
        "dbname=target",
    ])
    .unwrap();
    assert!(cli.tables.is_none());
}

#[test]
fn tables_accept_one_comma_separated_list() {
    let cli = Cli::try_parse_from([
        "pgex",
        "--source",
        "dbname=app",
        "--target",
        "dbname=target",
        "--tables",
        "public.events,audit.documents",
    ])
    .unwrap();
    assert_eq!(cli.tables.unwrap().len(), 2);
}

#[test]
fn tables_reject_unqualified_names() {
    let error = Cli::try_parse_from([
        "pgex",
        "--source",
        "dbname=app",
        "--target",
        "dbname=target",
        "--tables",
        "events",
    ])
    .err()
    .unwrap();
    assert_eq!(error.kind(), ErrorKind::ValueValidation);
}

#[test]
fn omitted_snapshot_disconnect_check_interval_is_unspecified() {
    let cli = Cli::try_parse_from([
        "pgex",
        "--source",
        "dbname=app",
        "--target",
        "dbname=target",
    ])
    .unwrap();
    assert_eq!(cli.client_connection_check_interval_ms, None);
}

#[test]
fn snapshot_disconnect_check_interval_can_be_overridden_or_disabled() {
    for (value, expected) in [("250", 250), ("0", 0)] {
        let cli = Cli::try_parse_from([
            "pgex",
            "--source",
            "dbname=app",
            "--target",
            "dbname=target",
            "--client-connection-check-interval-ms",
            value,
        ])
        .unwrap();
        assert_eq!(cli.client_connection_check_interval_ms, Some(expected));
    }
}

#[test]
fn snapshot_disconnect_check_interval_must_fit_a_nonnegative_postgresql_integer() {
    for value in ["-1", "2147483648"] {
        let flag = format!("--client-connection-check-interval-ms={value}");
        let error = Cli::try_parse_from([
            "pgex",
            "--source",
            "dbname=app",
            "--target",
            "dbname=target",
            &flag,
        ])
        .err()
        .unwrap();
        assert_eq!(error.kind(), ErrorKind::ValueValidation);
    }
}

#[test]
fn source_snapshot_timeouts_preserve_absence_and_explicit_zero() {
    let args = [
        "pgex",
        "--source",
        "dbname=source",
        "--target",
        "dbname=target",
    ];
    let timeouts = |cli: Cli| {
        [
            cli.source_exporter_transaction_timeout_ms,
            cli.source_exporter_idle_in_transaction_timeout_ms,
            cli.source_reader_transaction_timeout_ms,
            cli.source_reader_idle_in_transaction_timeout_ms,
        ]
    };
    assert_eq!(timeouts(Cli::try_parse_from(args).unwrap()), [None; 4]);

    let flags = [
        "--source-exporter-transaction-timeout-ms=900000",
        "--source-exporter-idle-in-transaction-timeout-ms=800000",
        "--source-reader-transaction-timeout-ms=120000",
        "--source-reader-idle-in-transaction-timeout-ms=0",
    ];
    let cli = Cli::try_parse_from(args.into_iter().chain(flags)).unwrap();
    assert_eq!(
        timeouts(cli),
        [Some(900_000), Some(800_000), Some(120_000), Some(0)]
    );
}

#[test]
fn source_snapshot_timeouts_must_fit_postgresql_milliseconds() {
    for option in [
        "--source-exporter-transaction-timeout-ms",
        "--source-exporter-idle-in-transaction-timeout-ms",
        "--source-reader-transaction-timeout-ms",
        "--source-reader-idle-in-transaction-timeout-ms",
    ] {
        for value in ["-1", "2147483648"] {
            let flag = format!("{option}={value}");
            let error = Cli::try_parse_from([
                "pgex",
                "--source",
                "dbname=source",
                "--target",
                "dbname=target",
                &flag,
            ])
            .err()
            .unwrap();
            assert_eq!(error.kind(), ErrorKind::ValueValidation);
        }
    }
}

#[test]
fn explicit_password_skips_environment_lookup() {
    let password = resolve_password(Some("explicit".into()), "PGEX_SOURCE_PASSWORD", || {
        panic!("the CLI password takes precedence")
    })
    .unwrap();
    assert_eq!(password.as_deref(), Some("explicit"));
}

#[test]
fn environment_password_is_used_when_the_option_is_omitted() {
    let password =
        resolve_password(None, "PGEX_SOURCE_PASSWORD", || Ok("environment".into())).unwrap();
    assert_eq!(password.as_deref(), Some("environment"));
}

#[test]
fn missing_environment_password_leaves_the_connection_string_in_control() {
    let password =
        resolve_password(None, "PGEX_SOURCE_PASSWORD", || Err(VarError::NotPresent)).unwrap();
    assert!(password.is_none());
}

#[test]
fn invalid_environment_password_is_rejected() {
    let error = resolve_password(None, "PGEX_SOURCE_PASSWORD", || {
        Err(VarError::NotUnicode("invalid".into()))
    })
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "PGEX_SOURCE_PASSWORD must be valid UTF-8"
    );
}
