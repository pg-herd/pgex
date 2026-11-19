use pgex::connection::Row;
use std::path::Path;

use anyhow::Result;
use tokio::process::Command;

use super::{postgres::PostgresTestDatabase, program::pgex_binary};

#[tokio::test]
async fn cli_displays_help_without_connection_settings() -> Result<()> {
    for (arguments, expected_code) in [(&[][..], 2), (&["-h"][..], 0), (&["--help"][..], 0)] {
        let output = Command::new(pgex_binary()).args(arguments).output().await?;
        assert_eq!(output.status.code(), Some(expected_code), "{arguments:?}");
        let (help, other) = if expected_code == 0 {
            (&output.stdout, &output.stderr)
        } else {
            (&output.stderr, &output.stdout)
        };
        assert!(other.is_empty(), "{arguments:?}: {other:?}");
        let help = String::from_utf8_lossy(help);
        for expected in [
            "Usage: pgex",
            "--config",
            "--source",
            "--target",
            "Examples:",
            "pgex --config pgex.yml",
            "PGEX_SOURCE_PASSWORD",
            "PGEX_TARGET_PASSWORD",
            "Selection:",
            "Source:",
            "Destination:",
            "Run pgex --help to see all options.",
        ] {
            assert!(help.contains(expected), "Missing {expected:?}: {help}");
        }
        assert!(!help.contains("Missing source.connection.url"), "{help}");
        for line in help.lines() {
            assert!(line.len() <= 80, "Help exceeds 80 columns: {line}");
        }
        if arguments == ["--help"] {
            for expected in [
                "Transfers:",
                "Snapshot:",
                "Timeouts:",
                "--source <URL>\n",
                "--target <URL>\n",
                "--source-exporter-transaction-timeout-ms <MS>\n",
                "--target-truncate",
            ] {
                assert!(help.contains(expected), "Missing {expected:?}: {help}");
            }
        } else {
            assert!(help.lines().count() <= 35, "Brief help is too long: {help}");
            assert!(!help.contains("--source-exporter-transaction-timeout-ms"));
            assert!(!help.contains("--target-truncate"));
        }
    }
    Ok(())
}

#[tokio::test]
async fn cli_and_yaml_users_take_precedence_on_each_connection() -> Result<()> {
    let (source, target) =
        tokio::try_join!(PostgresTestDatabase::start(), PostgresTestDatabase::start())?;
    let source_session = source.connector().await?.connect().await?;
    let target_session = target.connector().await?.connect().await?;
    let schema = "CREATE TABLE public.events (id integer PRIMARY KEY)";
    source_session.client().batch_execute(schema).await?;
    target_session.client().batch_execute(schema).await?;
    source_session
        .client()
        .execute("INSERT INTO public.events VALUES (0)", &[])
        .await?;

    #[derive(Clone, Copy, Debug)]
    enum Username {
        Url,
        Yaml,
        Cli,
    }

    let config = tempfile::NamedTempFile::new()?;
    let connection = |database: &PostgresTestDatabase, username| {
        let url = &database.connection_string;
        let missing_user_url = || url.replacen("pgex_test@", "missing_url_user@", 1);
        match username {
            Username::Url => format!("    url: '{url}'\n"),
            Username::Yaml => format!("    url: '{}'\n    user: pgex_test\n", missing_user_url()),
            Username::Cli => format!(
                "    url: '{}'\n    user: missing_yaml_user\n",
                missing_user_url()
            ),
        }
    };

    for (id, source_user, target_user) in [
        (1, Username::Yaml, Username::Url),
        (2, Username::Url, Username::Yaml),
        (3, Username::Cli, Username::Url),
        (4, Username::Url, Username::Cli),
    ] {
        source_session
            .client()
            .execute("UPDATE public.events SET id = $1", &[&id])
            .await?;
        std::fs::write(
            config.path(),
            format!(
                "source:\n  connection:\n{}  selection:\n    tables: [public.events]\ntarget:\n  connection:\n{}",
                connection(&source, source_user),
                connection(&target, target_user),
            ),
        )?;

        let mut command = config_command(config.path(), &source, &target);
        command
            .env("PGEX_SOURCE_PASSWORD", &source.password)
            .env("PGEX_TARGET_PASSWORD", &target.password);
        if matches!(source_user, Username::Cli) {
            command.args(["--source-user", "pgex_test"]);
        }
        if matches!(target_user, Username::Cli) {
            command.args(["--target-user", "pgex_test"]);
        }
        let output = command.output().await?;
        assert!(
            output.status.success(),
            "source username from {source_user:?}, target username from {target_user:?}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let row = target_session
            .client()
            .query_one("SELECT id FROM public.events", &[])
            .await?;
        assert_eq!(row.get::<i32, _>(0), id);
    }

    source_session.close().await;
    target_session.close().await;
    Ok(())
}

#[tokio::test]
async fn password_precedence_applies_independently_to_source_and_target() -> Result<()> {
    let (source, target) =
        tokio::try_join!(PostgresTestDatabase::start(), PostgresTestDatabase::start())?;
    let source_session = source.connector().await?.connect().await?;
    let target_session = target.connector().await?.connect().await?;
    let schema = "CREATE TABLE public.events (id integer PRIMARY KEY)";
    source_session.client().batch_execute(schema).await?;
    target_session.client().batch_execute(schema).await?;
    source_session
        .client()
        .execute("INSERT INTO public.events VALUES (0)", &[])
        .await?;

    #[derive(Clone, Copy, Debug)]
    enum Password {
        Cli,
        Yaml,
        Environment,
        Url,
    }

    let connection = |database: &PostgresTestDatabase, password| {
        let url_password = match password {
            Password::Url => database.password.as_str(),
            _ => "invalid_url_password",
        };
        let url = database.connection_string.replacen(
            "pgex_test@",
            &format!("pgex_test:{url_password}@"),
            1,
        );
        let yaml_password = match password {
            Password::Cli => "    password: 'invalid_yaml_password'\n".to_owned(),
            Password::Yaml => format!("    password: '{}'\n", database.password),
            Password::Environment | Password::Url => String::new(),
        };
        format!("    url: '{url}'\n{yaml_password}")
    };

    let set_password =
        |command: &mut Command, password, valid: &str, flag: &str, variable: &str| match password {
            Password::Cli => {
                command
                    .args([flag, valid])
                    .env(variable, "invalid_environment_password");
            }
            Password::Yaml => {
                command.env(variable, "invalid_environment_password");
            }
            Password::Environment => {
                command.env(variable, valid);
            }
            Password::Url => {
                command.env_remove(variable);
            }
        };

    let config = tempfile::NamedTempFile::new()?;
    for (id, source_password, target_password) in [
        (1, Password::Cli, Password::Yaml),
        (2, Password::Yaml, Password::Environment),
        (3, Password::Environment, Password::Url),
        (4, Password::Url, Password::Cli),
    ] {
        source_session
            .client()
            .execute("UPDATE public.events SET id = $1", &[&id])
            .await?;
        std::fs::write(
            config.path(),
            format!(
                "source:\n  connection:\n{}  selection:\n    tables: [public.events]\ntarget:\n  connection:\n{}",
                connection(&source, source_password),
                connection(&target, target_password),
            ),
        )?;

        let mut command = config_command(config.path(), &source, &target);
        set_password(
            &mut command,
            source_password,
            &source.password,
            "--source-password",
            "PGEX_SOURCE_PASSWORD",
        );
        set_password(
            &mut command,
            target_password,
            &target.password,
            "--target-password",
            "PGEX_TARGET_PASSWORD",
        );
        let output = command.output().await?;
        assert!(
            output.status.success(),
            "source password from {source_password:?}, target password from {target_password:?}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let row = target_session
            .client()
            .query_one("SELECT id FROM public.events", &[])
            .await?;
        assert_eq!(row.get::<i32, _>(0), id);
    }

    source_session.close().await;
    target_session.close().await;
    Ok(())
}

fn config_command(
    path: &Path,
    source: &PostgresTestDatabase,
    target: &PostgresTestDatabase,
) -> Command {
    let mut command = Command::new(pgex_binary());
    command
        .arg("--config")
        .arg(path)
        .arg("--ca")
        .arg(source.ca.path())
        .arg("--target-ca")
        .arg(target.ca.path());
    command
}

#[tokio::test]
async fn cli_validates_destination_layouts_and_reports_mismatches() -> Result<()> {
    let source = PostgresTestDatabase::start().await?;
    let target = PostgresTestDatabase::start().await?;
    let source_session = source.connector().await?.connect().await?;
    let target_session = target.connector().await?.connect().await?;
    let layout = "CREATE TYPE public.priority AS ENUM ('low', 'high');
                  CREATE TABLE public.events (company_id bigint, priority public.priority NOT NULL);";
    source_session.client().batch_execute(layout).await?;
    target_session.client().batch_execute(layout).await?;

    let run = || {
        let mut command = Command::new(pgex_binary());
        command
            .arg("--source")
            .arg(&source.connection_string)
            .arg("--ca")
            .arg(source.ca.path())
            .env("PGEX_SOURCE_PASSWORD", &source.password)
            .arg("--target")
            .arg(&target.connection_string)
            .arg("--target-ca")
            .arg(target.ca.path())
            .env("PGEX_TARGET_PASSWORD", &target.password)
            .args(["--tables", "public.events"]);
        command
    };

    let matching = run().output().await?;
    assert!(
        matching.status.success(),
        "{}",
        String::from_utf8_lossy(&matching.stderr)
    );
    assert_eq!(matching.stdout, b"Sync complete.\n");

    target_session
        .client()
        .batch_execute(
            "DROP TABLE public.events;
         DROP TYPE public.priority;
         CREATE TYPE public.priority AS ENUM ('high', 'low');
         CREATE TABLE public.events (company_id bigint, priority public.priority NOT NULL);",
        )
        .await?;
    let mismatch = run().arg("--no-target-locks").output().await?;
    assert!(!mismatch.status.success());
    let error = String::from_utf8_lossy(&mismatch.stderr);
    assert!(
        error.contains(r#"Layout mismatch for "public"."events""#),
        "{error}"
    );
    assert!(
        error.contains(r#"Column "priority" type differs"#),
        "{error}"
    );
    assert!(
        error.contains(r#"["low", "high"]"#) && error.contains(r#"["high", "low"]"#),
        "{error}"
    );

    source_session
        .client()
        .execute("INSERT INTO public.events VALUES (1, 'low')", &[])
        .await?;
    target_session
        .client()
        .execute("INSERT INTO public.events VALUES (1, 'high')", &[])
        .await?;
    let validation_disabled = run()
        .args(["--target-validate-layouts", "false"])
        .output()
        .await?;
    assert!(
        validation_disabled.status.success(),
        "{}",
        String::from_utf8_lossy(&validation_disabled.stderr)
    );
    let row = target_session
        .client()
        .query_one("SELECT priority::text FROM public.events", &[])
        .await?;
    assert_eq!(row.get::<String, _>(0), "low");

    target_session
        .client()
        .batch_execute("DROP TABLE public.events")
        .await?;
    let missing = run().output().await?;
    assert!(!missing.status.success());
    let error = String::from_utf8_lossy(&missing.stderr);
    assert!(
        error.contains("Run with destination tables locked"),
        "{error}"
    );
    assert!(
        error.contains(r#"Table "public"."events" does not exist"#),
        "{error}"
    );
    source_session.close().await;
    target_session.close().await;
    Ok(())
}

#[tokio::test]
async fn cli_rejects_a_missing_destination() -> Result<()> {
    let output = Command::new(pgex_binary())
        .args(["--source", "dbname=source"])
        .output()
        .await?;

    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("target.connection.url"), "{error}");
    Ok(())
}

#[tokio::test]
async fn cli_reports_when_a_selected_relation_has_no_table_layout() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let session = database.connector().await?.connect().await?;
    session
        .client()
        .batch_execute("CREATE VIEW public.events_view AS SELECT 1 AS id")
        .await?;
    session.close().await;

    let output = Command::new(pgex_binary())
        .arg("--source")
        .arg(&database.connection_string)
        .arg("--ca")
        .arg(database.ca.path())
        .env("PGEX_SOURCE_PASSWORD", &database.password)
        .args(["--target", "dbname=target"])
        .args(["--tables", "public.events_view"])
        .output()
        .await?;

    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains(r#"No table layout found for "public"."events_view""#),
        "{error}"
    );
    Ok(())
}

#[tokio::test]
async fn cli_names_the_supplied_table_when_its_schema_is_missing() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let output = Command::new(pgex_binary())
        .arg("--source")
        .arg(&database.connection_string)
        .arg("--ca")
        .arg(database.ca.path())
        .env("PGEX_SOURCE_PASSWORD", &database.password)
        .args(["--target", "dbname=target"])
        .args(["--tables", "missing_schema.events,public.missing_table"])
        .output()
        .await?;

    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains(r#"Table "missing_schema"."events" does not exist"#),
        "{error}"
    );
    Ok(())
}

#[tokio::test]
async fn omitted_table_list_locks_discovered_tables_but_explicit_list_only_locks_selected_tables()
-> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let blocker = database.connector().await?.connect().await?;
    blocker
        .client()
        .batch_execute(
            "CREATE SCHEMA audit;
             CREATE TABLE audit.documents (company_id bigint);
             CREATE TABLE public.events (company_id bigint);",
        )
        .await?;
    blocker
        .client()
        .batch_execute("BEGIN; LOCK TABLE audit.documents IN ACCESS EXCLUSIVE MODE")
        .await?;

    let connection_string = format!(
        "{}?options=-c%20lock_timeout%3D1s",
        database.connection_string
    );
    let run = || {
        let mut command = Command::new(pgex_binary());
        command
            .arg("--source")
            .arg(&connection_string)
            .arg("--ca")
            .arg(database.ca.path())
            .env("PGEX_SOURCE_PASSWORD", &database.password)
            .arg("--target")
            .arg(&connection_string)
            .arg("--target-ca")
            .arg(database.ca.path())
            .env("PGEX_TARGET_PASSWORD", &database.password);
        command
    };

    let explicit = run().args(["--tables", "public.events"]).output().await?;
    let discovered = run().output().await?;
    blocker.client().batch_execute("ROLLBACK").await?;
    blocker.close().await;

    assert!(
        explicit.status.success(),
        "{}",
        String::from_utf8_lossy(&explicit.stderr)
    );
    assert_eq!(explicit.stdout, b"Sync complete.\n");
    assert!(!discovered.status.success());
    let error = String::from_utf8_lossy(&discovered.stderr);
    assert!(error.contains("lock timeout"), "{error}");
    Ok(())
}

#[tokio::test]
async fn ambient_pgpassword_does_not_replace_missing_pgex_credentials() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let output = Command::new(pgex_binary())
        .args([
            "--source",
            &database.connection_string,
            "--target",
            &database.connection_string,
        ])
        .arg("--ca")
        .arg(database.ca.path())
        .arg("--target-ca")
        .arg(database.ca.path())
        .args(["--tables", "public.missing"])
        .env_remove("PGEX_SOURCE_PASSWORD")
        .env_remove("PGEX_TARGET_PASSWORD")
        .env("PGPASSWORD", &database.password)
        .output()
        .await?;
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(error.contains("no password supplied"), "{error}");
    Ok(())
}
