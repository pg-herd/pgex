use crate::postgres::sql_state;
use pgex::connection::Row;
use std::{process::Stdio, time::Duration};

use anyhow::{Context, Result};
use pgex::connection::Client;
use tokio::process::Command;

use super::{postgres::PostgresTestDatabase, program::pgex_binary};

#[tokio::test]
async fn rolls_back_on_the_same_connection_after_copy_in_startup_fails() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let session = database.connector().await?.connect().await?;
    let client = session.client();
    client
        .batch_execute(
            "CREATE TABLE public.items (id integer); INSERT INTO public.items VALUES (1)",
        )
        .await?;

    client
        .batch_execute("BEGIN; DELETE FROM public.items")
        .await?;
    let error = {
        let mut connection = client.connection().await?;
        connection
            .get()
            .await?
            .copy_in_raw("COPY public.missing FROM STDIN")
            .await
            .err()
    }
    .context("COPY should fail at startup for a missing table")?;
    assert_eq!(sql_state(&error).as_deref(), Some("42P01"));
    client.batch_execute("ROLLBACK").await?;
    let id: i32 = client
        .query_one("SELECT id FROM public.items", &[])
        .await?
        .get(0);
    assert_eq!(id, 1);

    session.close().await;
    Ok(())
}

#[tokio::test]
async fn rolls_back_on_same_connection_after_aborting_copy_sink() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let session = database.connector().await?.connect().await?;
    let client = session.client();
    client
        .batch_execute(
            "CREATE TABLE public.items (id integer); INSERT INTO public.items VALUES (1)",
        )
        .await?;
    client
        .batch_execute("BEGIN; DELETE FROM public.items")
        .await?;
    {
        let mut connection = client.connection().await?;
        let mut sink = connection
            .get()
            .await?
            .copy_in_raw("COPY public.items FROM STDIN (FORMAT TEXT)")
            .await?;
        sink.send(bytes::Bytes::from_static(b"2\n")).await?;
        sink.abort("Intentional COPY cancellation").await?;
    }
    tokio::time::timeout(Duration::from_secs(5), client.batch_execute("ROLLBACK")).await??;
    let id: i32 = client
        .query_one("SELECT id FROM public.items", &[])
        .await?
        .get(0);
    assert_eq!(id, 1);
    session.close().await;
    Ok(())
}

#[tokio::test]
async fn closing_after_failed_copy_out_startup_rolls_back_the_transaction() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let connector = database.connector().await?;
    let observer = connector.connect().await?;
    observer
        .client()
        .batch_execute(
            "CREATE TABLE public.items (id integer); INSERT INTO public.items VALUES (1)",
        )
        .await?;
    let session = connector.connect().await?;
    session
        .client()
        .batch_execute("BEGIN; DELETE FROM public.items")
        .await?;
    let error = {
        let mut connection = session.client().connection().await?;
        connection
            .get()
            .await?
            .copy_out_raw("COPY public.missing TO STDOUT")
            .await
            .err()
    }
    .context("Expected source COPY startup to fail")?;
    assert_eq!(sql_state(&error).as_deref(), Some("42P01"));
    // Closing a failed source COPY session must roll back its transaction
    // and release the locks even if no COPY stream was returned.
    session.close().await;
    let id: i32 = observer
        .client()
        .query_one("SELECT id FROM public.items", &[])
        .await?
        .get(0);
    assert_eq!(id, 1);
    Ok(())
}

#[tokio::test]
async fn dropping_session_with_unfinished_copy_restores_destination_rows() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let connector = database.connector().await?;
    let observer = connector.connect().await?;
    observer
        .client()
        .batch_execute(
            "CREATE TABLE public.items (id integer); INSERT INTO public.items VALUES (1)",
        )
        .await?;
    let session = connector.connect().await?;
    session
        .client()
        .batch_execute("BEGIN; DELETE FROM public.items")
        .await?;
    {
        let mut connection = session.client().connection().await?;
        let mut copy = connection
            .get()
            .await?
            .copy_in_raw("COPY public.items FROM STDIN")
            .await?;
        copy.send(bytes::Bytes::from_static(b"2\n")).await?;
    }
    drop(session);
    let id: i32 = tokio::time::timeout(Duration::from_secs(5), async {
        observer
            .client()
            .query_one("SELECT id FROM public.items FOR UPDATE", &[])
            .await
            .map(|row| row.get(0))
    })
    .await??;
    assert_eq!(id, 1);
    Ok(())
}

#[tokio::test]
async fn uses_shared_snapshot_by_default_and_applies_explicit_snapshot_modes() -> Result<()> {
    let (source, target) =
        tokio::try_join!(PostgresTestDatabase::start(), PostgresTestDatabase::start())?;
    let source_session = source.connector().await?.connect().await?;
    let target_session = target.connector().await?.connect().await?;
    let schema = "CREATE TABLE public.items (id integer PRIMARY KEY, isolation text)";
    source_session.client().batch_execute(schema).await?;
    target_session.client().batch_execute(schema).await?;
    source_session
        .client()
        .batch_execute(
            "INSERT INTO public.items VALUES (1, 'read committed'), (2, 'repeatable read')",
        )
        .await?;

    let config = tempfile::NamedTempFile::new()?;
    std::fs::write(config.path(), "source: {snapshot: {mode: per-statement}}")?;
    let config_path = config.path().to_str().context("Read test config path")?;

    for (snapshot_args, expected_id) in [
        (&[][..], 2),
        (&["--source-snapshot", "per-statement"][..], 1),
        (&["--config", config_path][..], 1),
        (
            &["--config", config_path, "--source-snapshot", "shared"][..],
            2,
        ),
    ] {
        target_session
            .client()
            .batch_execute("TRUNCATE public.items")
            .await?;
        let output = sync_command(&source, &source.connection_string, &target)
            .args([
                "--tables",
                "public.items",
                "--where",
                "isolation = pg_catalog.current_setting('transaction_isolation')",
                "--target-skip-delete",
            ])
            .args(snapshot_args)
            .output()
            .await?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let rows = target_session
            .client()
            .query("SELECT id FROM public.items ORDER BY id", &[])
            .await?;
        let ids: Vec<i32> = rows.iter().map(|row| row.get(0)).collect();
        assert_eq!(ids, [expected_id]);
    }

    source_session.close().await;
    target_session.close().await;
    Ok(())
}

#[tokio::test]
async fn applies_a_delete_filter_separately_from_a_copy_predicate() -> Result<()> {
    let (source, target) =
        tokio::try_join!(PostgresTestDatabase::start(), PostgresTestDatabase::start())?;
    let source_session = source.connector().await?.connect().await?;
    let target_session = target.connector().await?.connect().await?;
    let schema = "CREATE TABLE public.items (id integer PRIMARY KEY, label text)";
    source_session.client().batch_execute(schema).await?;
    target_session.client().batch_execute(schema).await?;
    let label = r"O'Reilly\notes";
    source_session
        .client()
        .execute(
            "INSERT INTO public.items VALUES (1, $1), (2, 'excluded')",
            &[&label],
        )
        .await?;
    target_session
        .client()
        .batch_execute("INSERT INTO public.items VALUES (1, 'stale'), (3, 'remove'), (4, 'keep')")
        .await?;
    let config = tempfile::NamedTempFile::new()?;
    std::fs::write(
        config.path(),
        "source: {selection: {tables: [public.items]}}\ntarget: {delete_filters: {public.items: 'id <= {{delete_through:integer}}'}}",
    )?;

    let output = sync_command(&source, &source.connection_string, &target)
        .arg("--config")
        .arg(config.path())
        .args([
            "--where",
            "label = {{label}} -- trailing comment",
            "--param",
            &format!("label={label}"),
            "--param",
            "delete_through=3",
        ])
        .output()
        .await?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows = target_session
        .client()
        .query("SELECT id, label FROM public.items ORDER BY id", &[])
        .await?;
    let values: Vec<_> = rows
        .iter()
        .map(|row| (row.get::<i32, _>(0), row.get::<&str, _>(1)))
        .collect();
    assert_eq!(values, [(1, label), (4, "keep")]);
    source_session.close().await;
    target_session.close().await;
    Ok(())
}

#[tokio::test]
async fn refreshes_the_entire_table_without_a_filter_or_company_column() -> Result<()> {
    let (source, target) =
        tokio::try_join!(PostgresTestDatabase::start(), PostgresTestDatabase::start())?;
    let source_session = source.connector().await?.connect().await?;
    let target_session = target.connector().await?.connect().await?;
    let schema = "CREATE TABLE public.items (id integer PRIMARY KEY, label text)";
    source_session.client().batch_execute(schema).await?;
    target_session.client().batch_execute(schema).await?;
    source_session
        .client()
        .batch_execute("INSERT INTO public.items VALUES (1, 'first'), (2, 'second')")
        .await?;
    target_session
        .client()
        .batch_execute("INSERT INTO public.items VALUES (1, 'stale'), (3, 'destination only')")
        .await?;

    let output = sync_command(&source, &source.connection_string, &target)
        .args(["--tables", "public.items"])
        .output()
        .await?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows = target_session
        .client()
        .query("SELECT id, label FROM public.items ORDER BY id", &[])
        .await?;
    let values: Vec<_> = rows
        .iter()
        .map(|row| (row.get::<i32, _>(0), row.get::<&str, _>(1)))
        .collect();
    assert_eq!(values, [(1, "first"), (2, "second")]);
    source_session.close().await;
    target_session.close().await;
    Ok(())
}

#[tokio::test]
async fn refreshes_filtered_rows_and_rolls_back_the_group_if_copy_fails() -> Result<()> {
    let (source, target) =
        tokio::try_join!(PostgresTestDatabase::start(), PostgresTestDatabase::start())?;
    let source_session = source.connector().await?.connect().await?;
    let target_session = target.connector().await?.connect().await?;
    let schema = r#"
        CREATE TABLE public.a_events (
            company_id integer, payload text, payload_length integer
                GENERATED ALWAYS AS (length(payload)) STORED
        );
        CREATE TABLE public.b_events (LIKE public.a_events INCLUDING GENERATED);
    "#;
    source_session.client().batch_execute(schema).await?;
    target_session.client().batch_execute(schema).await?;
    source_session
        .client()
        .batch_execute(
            "INSERT INTO public.a_events (company_id, payload)
             VALUES (1, repeat('x', 600000)), (2, 'source two'), (3, 'source three');
         INSERT INTO public.b_events (company_id, payload) VALUES (1, 'source one');
         ANALYZE public.a_events;
         ANALYZE public.b_events;",
        )
        .await?;
    target_session
        .client()
        .batch_execute(
            "INSERT INTO public.a_events (company_id, payload)
             VALUES (1, 'old one'), (2, 'old two'), (3, 'keep three');
         INSERT INTO public.b_events (company_id, payload) VALUES (1, 'old one');
         ALTER TABLE public.b_events ADD CHECK (payload <> 'source one');",
        )
        .await?;

    let run = || {
        let mut command = sync_command(&source, &source.connection_string, &target);
        command.args([
            "--where",
            "company_id = ANY({{ids:integer[]}})",
            "--param",
            "ids=1,2",
        ]);
        command
    };
    let read_rows = async || -> Result<Vec<(i32, String, i32)>> {
        let rows = target_session
            .client()
            .query(
                "SELECT company_id, payload, payload_length FROM public.a_events
             UNION ALL
             SELECT company_id + 10, payload, payload_length FROM public.b_events
             ORDER BY 1",
                &[],
            )
            .await?;
        rows.into_iter()
            .map(|row| Ok((row.try_get(0)?, row.try_get(1)?, row.try_get(2)?)))
            .collect()
    };

    let original = read_rows().await?;
    let failed = run().output().await?;
    assert!(!failed.status.success());
    let error = String::from_utf8_lossy(&failed.stderr);
    assert!(
        error.contains("Finish destination COPY") && error.contains("check constraint"),
        "{error}"
    );
    assert_eq!(read_rows().await?, original);

    target_session
        .client()
        .batch_execute("ALTER TABLE public.b_events DROP CONSTRAINT b_events_payload_check")
        .await?;
    let success = run().output().await?;
    assert!(
        success.status.success(),
        "{}",
        String::from_utf8_lossy(&success.stderr)
    );
    assert_eq!(
        read_rows().await?,
        [
            (1, "x".repeat(600000), 600000),
            (2, "source two".into(), 10),
            (3, "keep three".into(), 10),
            (11, "source one".into(), 10),
        ]
    );
    source_session.close().await;
    target_session.close().await;
    Ok(())
}

#[tokio::test]
async fn skip_delete_preserves_destination_rows_and_inserts_selected_rows() -> Result<()> {
    let (source, target) =
        tokio::try_join!(PostgresTestDatabase::start(), PostgresTestDatabase::start())?;
    let source_session = source.connector().await?.connect().await?;
    let target_session = target.connector().await?.connect().await?;
    let schema =
        "CREATE TABLE public.events (company_id integer, id integer, PRIMARY KEY (company_id, id))";
    source_session.client().batch_execute(schema).await?;
    target_session.client().batch_execute(schema).await?;
    source_session
        .client()
        .batch_execute("INSERT INTO public.events VALUES (1, 10), (2, 20)")
        .await?;
    target_session
        .client()
        .batch_execute("INSERT INTO public.events VALUES (1, 11), (2, 21)")
        .await?;

    let mut command = sync_command(&source, &source.connection_string, &target);
    command.args([
        "--tables",
        "public.events",
        "--where",
        "company_id = 1",
        "--target-skip-delete",
    ]);
    let output = command.output().await?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let duplicate = command.output().await?;
    assert!(!duplicate.status.success());
    let error = String::from_utf8_lossy(&duplicate.stderr);
    assert!(error.contains("duplicate key"), "{error}");
    let rows = target_session
        .client()
        .query("SELECT id FROM public.events ORDER BY id", &[])
        .await?;
    let ids: Vec<i32> = rows.iter().map(|row| row.get(0)).collect();
    assert_eq!(ids, [10, 11, 21]);
    Ok(())
}

#[tokio::test]
async fn truncate_and_filtered_copy_commit_or_roll_back_together() -> Result<()> {
    let (source, mut target) =
        tokio::try_join!(PostgresTestDatabase::start(), PostgresTestDatabase::start())?;
    let source_session = source.connector().await?.connect().await?;
    let target_session = target.connector().await?.connect().await?;
    source_session
        .client()
        .batch_execute(
            "CREATE TABLE public.items (id integer PRIMARY KEY);
                        INSERT INTO public.items VALUES (1), (2)",
        )
        .await?;
    let target_client = target_session.client();
    target_client
        .batch_execute(
            "CREATE TABLE public.items (id integer PRIMARY KEY);
                        CREATE TABLE public.children (item_id integer REFERENCES public.items);
                        INSERT INTO public.items VALUES (9);
                        INSERT INTO public.children VALUES (9)",
        )
        .await?;

    target
        .connection_string
        .push_str("?options=-c%20log_statement%3Dall");
    let run = || {
        let mut command = sync_command(&source, &source.connection_string, &target);
        command.args([
            "--tables",
            "public.items",
            "--where",
            "id = 1",
            "--target-truncate",
        ]);
        command
    };
    let result = run().output().await?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let row = target_client
        .query_one(
            "SELECT ARRAY(SELECT id FROM public.items), (SELECT count(*) FROM public.children)",
            &[],
        )
        .await?;
    assert_eq!(row.get::<Vec<i32>, _>(0), [1]);
    assert_eq!(row.get::<i64, _>(1), 0);
    let log = target.stderr().await?;
    let truncate = log
        .find(r#"statement: TRUNCATE "public"."items" CASCADE"#)
        .context("Expected simple-query TRUNCATE")?;
    assert!(
        log[truncate..].contains(r#"statement: COPY "public"."items""#),
        "{log}"
    );

    target_client
        .batch_execute(
            "UPDATE public.items SET id = 9;
                        INSERT INTO public.children VALUES (9);
                        ALTER TABLE public.items ADD CHECK (id > 8)",
        )
        .await?;
    for replica_role in [false, true] {
        let mut command = run();
        if replica_role {
            command.arg("--target-replica-role");
        }
        let result = command.output().await?;
        assert!(!result.status.success());
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(error.contains("violates check constraint"), "{error}");
        let remaining = target_client
            .query_one(
                "SELECT ARRAY(SELECT id FROM public.items), ARRAY(SELECT item_id FROM public.children)",
                &[],
            )
            .await?;
        assert_eq!(remaining.get::<Vec<i32>, _>(0), [9]);
        assert_eq!(remaining.get::<Vec<i32>, _>(1), [9]);
    }
    Ok(())
}

#[tokio::test]
async fn exporter_timeout_during_copy_closes_workers_and_restores_destination_rows() -> Result<()> {
    let (source, target) =
        tokio::try_join!(PostgresTestDatabase::start(), PostgresTestDatabase::start())?;
    let source_session = source.connector().await?.connect().await?;
    let target_session = target.connector().await?.connect().await?;
    let source_observer = source_session.client();
    let target_observer = target_session.client();
    let schema = "CREATE TABLE public.events (company_id integer, id integer, payload text)";
    source_observer.batch_execute(schema).await?;
    target_observer.batch_execute(schema).await?;
    source_observer
        .batch_execute(
            "INSERT INTO public.events SELECT 1, id, repeat('x', 8192)
                FROM generate_series(1, 128) AS id;
             CREATE ROLE slow_copy_reader;
             GRANT SELECT ON public.events TO slow_copy_reader;
             CREATE FUNCTION public.pause_copy(id integer) RETURNS boolean LANGUAGE plpgsql AS $$
             BEGIN
                 IF id = 97 THEN
                     PERFORM pg_sleep(30);
                 END IF;
                 RETURN true;
             END
             $$;
             ALTER TABLE public.events ENABLE ROW LEVEL SECURITY;
             CREATE POLICY pause_copy ON public.events FOR SELECT TO slow_copy_reader
                 USING (public.pause_copy(id));",
        )
        .await?;
    // A BEFORE INSERT row trigger makes COPY insert rows as they arrive,
    // so pg_stat_progress_copy counts inserted rows before the stream ends.
    target_observer
        .batch_execute(
            "INSERT INTO public.events VALUES (1, -1, 'original'), (2, -2, 'other tenant');
             CREATE FUNCTION public.insert_one_at_a_time() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 RETURN NEW;
             END
             $$;
             CREATE TRIGGER insert_one_at_a_time BEFORE INSERT ON public.events
                 FOR EACH ROW EXECUTE FUNCTION public.insert_one_at_a_time();",
        )
        .await?;

    // Run the source COPY as a non-owner so its SELECT policy pauses the stream.
    let source_url = format!(
        "{}?options=-c%20role%3Dslow_copy_reader",
        source.connection_string
    );
    let child = sync_command(&source, &source_url, &target)
        .args(["--where", "company_id = 1"])
        .args([
            "--tables",
            "public.events",
            "--source-exporter-idle-in-transaction-timeout-ms",
            "4000",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let progress = wait_for_partial_copy(target_observer);
    let done = child.wait_with_output();
    tokio::pin!(done);
    tokio::select! {
        output = &mut done => anyhow::bail!("pgex exited before partial COPY: {}", String::from_utf8_lossy(&output?.stderr)),
        result = progress => result?,
    }
    let source_copy_is_paused: bool = source_observer
        .query_one(
            "SELECT EXISTS (
                 SELECT FROM pg_stat_activity
                 WHERE wait_event = 'PgSleep' AND query LIKE 'COPY % TO STDOUT %'
             )",
            &[],
        )
        .await?
        .try_get(0)?;
    assert!(
        source_copy_is_paused,
        "Expected source COPY to still be running"
    );
    let original_row = target_observer
        .query_one(
            "SELECT id FROM public.events WHERE company_id = 1 FOR UPDATE NOWAIT",
            &[],
        )
        .await;
    assert!(
        matches!(&original_row, Err(error) if sql_state(error).as_deref() == Some("55P03")),
        "Expected the original row to be locked by the worker's DELETE, got {original_row:?}"
    );

    let output = tokio::time::timeout(Duration::from_secs(8), done)
        .await
        .context("pgex kept running after the exporter timeout")??;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("Source coordinator session disconnected")
            && stderr.contains("idle-in-transaction timeout"),
        "{stderr}"
    );

    tokio::try_join!(
        wait_for_sync_sessions_to_close(source_observer),
        wait_for_sync_sessions_to_close(target_observer)
    )?;
    let rows = target_observer
        .query(
            "SELECT id, payload FROM public.events ORDER BY company_id",
            &[],
        )
        .await?;
    let rows: Vec<_> = rows
        .iter()
        .map(|row| (row.get::<i32, _>(0), row.get::<&str, _>(1)))
        .collect();
    assert_eq!(rows, [(-1, "original"), (-2, "other tenant")]);
    Ok(())
}

fn sync_command(
    source: &PostgresTestDatabase,
    source_url: &str,
    target: &PostgresTestDatabase,
) -> Command {
    let mut command = Command::new(pgex_binary());
    command
        .args([
            "--source",
            source_url,
            "--target",
            &target.connection_string,
        ])
        .arg("--ca")
        .arg(source.ca.path())
        .arg("--target-ca")
        .arg(target.ca.path())
        .env("PGEX_SOURCE_PASSWORD", &source.password)
        .env("PGEX_TARGET_PASSWORD", &target.password)
        .args(["--target-parallel-transfers", "1"]);
    command
}

async fn wait_for_partial_copy(observer: &Client) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let partial: bool = observer
                .query_one(
                    "SELECT EXISTS (
                         SELECT FROM pg_stat_progress_copy
                         WHERE relid = 'public.events'::regclass AND command = 'COPY FROM'
                           AND tuples_processed BETWEEN 1 AND 127
                     )",
                    &[],
                )
                .await?
                .try_get(0)?;
            if partial {
                return Ok::<_, pgex::connection::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .context("Destination COPY did not report partial progress")??;
    Ok(())
}

async fn wait_for_sync_sessions_to_close(observer: &Client) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let sessions: i64 = observer
                .query_one(
                    "SELECT count(*) FROM pg_stat_activity
                 WHERE datname = current_database()
                   AND backend_type = 'client backend' AND pid <> pg_backend_pid()",
                    &[],
                )
                .await?
                .try_get(0)?;
            if sessions == 0 {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .context("Sync sessions remained open after exporter timeout")?
}

#[tokio::test]
async fn syncs_quoted_catalog_names_with_literal_filters_and_truncate() -> Result<()> {
    let (source, target) =
        tokio::try_join!(PostgresTestDatabase::start(), PostgresTestDatabase::start())?;
    let source_session = source.connector().await?.connect().await?;
    let target_session = target.connector().await?.connect().await?;
    let schema = r#"CREATE SCHEMA "odd ""雪";
        CREATE TABLE "odd ""雪"."Mixed ' table" ("i""d" integer PRIMARY KEY, "la""bel" text)"#;
    source_session.client().batch_execute(schema).await?;
    target_session.client().batch_execute(schema).await?;
    let label = "O'Reilly\\notes 雪'; DROP TABLE nope; --";
    source_session
        .client()
        .execute(
            r#"INSERT INTO "odd ""雪"."Mixed ' table" VALUES (1, $1), (2, 'excluded')"#,
            &[&label],
        )
        .await?;
    for truncate in [false, true] {
        target_session.client().execute(
            r#"INSERT INTO "odd ""雪"."Mixed ' table" VALUES (1, $1) ON CONFLICT ("i""d") DO UPDATE SET "la""bel" = excluded."la""bel""#,
            &[&label],
        ).await?;
        let mut command = sync_command(&source, &source.connection_string, &target);
        command.args([
            "--where",
            r#""la""bel" = {{label:text}}"#,
            "--param",
            &format!("label={label}"),
        ]);
        // Ambient libpq identity variables must not change explicit credentials.
        command
            .env("PGUSER", "ambient_user_must_be_ignored")
            .env("PGDATABASE", "ambient_db_must_be_ignored");
        if truncate {
            command.arg("--target-truncate");
        }
        let output = command.output().await?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let rows = target_session
            .client()
            .query(
                r#"SELECT "i""d", "la""bel" FROM "odd ""雪"."Mixed ' table""#,
                &[],
            )
            .await?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<i32, _>(0), 1);
        assert_eq!(rows[0].get::<String, _>(1), label);
    }
    source_session.close().await;
    target_session.close().await;
    Ok(())
}

#[tokio::test]
async fn queued_source_ddl_fails_copy_before_destination_changes() -> Result<()> {
    let (source, target) =
        tokio::try_join!(PostgresTestDatabase::start(), PostgresTestDatabase::start())?;
    let connector = source.connector().await?;
    let observer = connector.connect().await?;
    let gate = connector.connect().await?;
    let target_session = target.connector().await?.connect().await?;
    let schema = "CREATE TABLE public.items(id int) PARTITION BY RANGE (id);
        CREATE TABLE public.items_leaf PARTITION OF public.items DEFAULT";
    observer.client().batch_execute(schema).await?;
    target_session.client().batch_execute(schema).await?;
    observer
        .client()
        .batch_execute("INSERT INTO public.items VALUES (42)")
        .await?;

    for (truncate, queued_relation) in [(false, "items"), (true, "items_leaf")] {
        target_session
            .client()
            .batch_execute("TRUNCATE public.items; INSERT INTO public.items VALUES (-1)")
            .await?;
        // A third session holds a compatible lock solely to keep the DDL queued.
        gate.client()
            .batch_execute(&format!(
                "BEGIN; LOCK TABLE ONLY public.{queued_relation} IN ACCESS SHARE MODE"
            ))
            .await?;
        let ddl = connector.connect().await?;
        let alteration = tokio::spawn(async move {
            ddl.client()
                .batch_execute(&format!(
                    "ALTER TABLE public.{queued_relation} ALTER COLUMN id SET DEFAULT 123"
                ))
                .await
        });
        waiting_relation_lock(observer.client(), queued_relation, "AccessExclusiveLock").await?;
        let mut command = sync_command(&source, &source.connection_string, &target);
        command.args(["--tables", "public.items"]);
        if truncate {
            command.args(["--target-truncate", "--source-snapshot", "per-statement"]);
        }
        let child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
            .await
            .context("COPY worker waited behind queued DDL")??;
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success());
        assert!(
            error.contains(r#"Acquire source COPY lock on "public"."items" without waiting"#)
                && error.contains("could not obtain lock"),
            "{error}"
        );
        gate.client().batch_execute("ROLLBACK").await?;
        tokio::time::timeout(Duration::from_secs(5), alteration).await???;
        let id: i32 = target_session
            .client()
            .query_one("SELECT id FROM public.items", &[])
            .await?
            .try_get(0)?;
        assert_eq!(id, -1);

        let retry = command.output().await?;
        assert!(
            retry.status.success(),
            "{}",
            String::from_utf8_lossy(&retry.stderr)
        );
        let id: i32 = target_session
            .client()
            .query_one("SELECT id FROM public.items", &[])
            .await?
            .try_get(0)?;
        assert_eq!(id, 42);
    }
    Ok(())
}

async fn waiting_relation_lock(observer: &Client, relation: &str, mode: &str) -> Result<i32> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let rows = observer
                .query(
                    "SELECT pid FROM pg_locks WHERE relation=to_regclass($1::text)
                 AND mode=$2::text AND NOT granted",
                    &[&relation, &mode],
                )
                .await?;
            if let Some(row) = rows.first() {
                return anyhow::Ok(row.try_get(0)?);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .with_context(|| format!("Waiting for {mode} on {relation}"))?
}
