use std::{process::Stdio, time::Duration};

use anyhow::{Context, Result};
use tokio::process::Command;
use tokio_postgres::{Client, error::SqlState};

use super::postgres::PostgresTestDatabase;

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
        .map(|row| (row.get::<_, i32>(0), row.get::<_, &str>(1)))
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
        .map(|row| (row.get::<_, i32>(0), row.get::<_, &str>(1)))
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
        matches!(&original_row, Err(error) if error.code() == Some(&SqlState::LOCK_NOT_AVAILABLE)),
        "Expected the original row to be locked by the worker's DELETE, got {original_row:?}"
    );

    let output = tokio::time::timeout(Duration::from_secs(8), done)
        .await
        .context("pgex kept running after the exporter timeout")??;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("Source snapshot exporter disconnected")
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
        .map(|row| (row.get::<_, i32>(0), row.get::<_, &str>(1)))
        .collect();
    assert_eq!(rows, [(-1, "original"), (-2, "other tenant")]);
    Ok(())
}

fn sync_command(
    source: &PostgresTestDatabase,
    source_url: &str,
    target: &PostgresTestDatabase,
) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pgex"));
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
                return Ok::<_, tokio_postgres::Error>(());
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
