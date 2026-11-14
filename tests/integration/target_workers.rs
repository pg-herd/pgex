use anyhow::Result;
use tokio::process::Command;

use super::postgres::PostgresTestDatabase;

#[tokio::test]
async fn worker_imports_once_and_reuses_one_destination_session_across_transactions() -> Result<()>
{
    let database = PostgresTestDatabase::start().await?;
    let setup = database.connector().await?.connect().await?;
    setup
        .client()
        .batch_execute(
            "CREATE TABLE public.a_first (company_id bigint, id integer, payload text)
                 WITH (autovacuum_enabled = false);
             CREATE TABLE public.b_second (LIKE public.a_first) WITH (autovacuum_enabled = false);
             CREATE TABLE public.c_oversized (LIKE public.a_first) WITH (autovacuum_enabled = false);
             CREATE TABLE public.d_last (LIKE public.a_first) WITH (autovacuum_enabled = false);
             INSERT INTO public.a_first VALUES (1, 1, repeat('x', 524241));
             INSERT INTO public.b_second VALUES (1, 1, repeat('x', 524241));
             INSERT INTO public.c_oversized VALUES (1, 1, repeat('x', 1100000));
             INSERT INTO public.d_last VALUES (1, 1, 'last');",
        )
        .await?;
    setup.close().await;

    // Use one server for both endpoints so its statement log shows all sessions.
    let connection_string = format!(
        "{}?options=-c%20log_statement%3Dall",
        database.connection_string
    );
    let output = Command::new(env!("CARGO_BIN_EXE_pgex"))
        .args([
            "--source",
            &connection_string,
            "--target",
            &connection_string,
        ])
        .arg("--ca")
        .arg(database.ca.path())
        .arg("--target-ca")
        .arg(database.ca.path())
        .env("PGEX_SOURCE_PASSWORD", &database.password)
        .env("PGEX_TARGET_PASSWORD", &database.password)
        .args([
            "--source-reader-limit",
            "1",
            "--target-parallel-transfers",
            "2",
            "--target-commit-group-mib",
            "1",
            "--where",
            "company_id = 1",
        ])
        .output()
        .await?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(
        stderr.contains("Warning: PostgreSQL target parallel transfers (2) exceed the source reader connection limit (1). Capping the worker count at 1."),
        "{stderr}"
    );
    assert_eq!(output.stdout, b"Sync complete.\n");

    let log = database.stderr().await?;
    let imports = statement_backends(&log, |sql| sql.starts_with("SET TRANSACTION SNAPSHOT "));
    // One import to read layouts and one for the worker's entire loop.
    assert_eq!(imports.len(), 2, "{log}");
    assert_ne!(imports[0], imports[1], "{log}");

    let begins = statement_backends(&log, |sql| sql == "BEGIN");
    let commits = statement_backends(&log, |sql| sql == "COMMIT");
    // Each of the first two tables sends 512 KiB (text plus 47 bytes of COPY
    // framing and row data). Together they hit 1 MiB exactly. The oversized
    // third table and the remaining fourth table each get their own transaction.
    assert_eq!(begins.len(), 4, "{log}");
    let lock_session = begins[0];
    let worker_session = begins[1];
    assert_ne!(lock_session, worker_session, "{log}");
    assert_eq!(
        begins,
        [lock_session, worker_session, worker_session, worker_session]
    );
    assert_eq!(
        commits,
        [worker_session, worker_session, worker_session, lock_session]
    );

    Ok(())
}

#[tokio::test]
async fn row_security_overrides_are_opt_in_and_independent() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let setup = database.connector().await?.connect().await?;
    setup
        .client()
        .batch_execute("CREATE TABLE public.events (company_id bigint, id integer)")
        .await?;
    setup.close().await;

    let connection_string = format!(
        "{}?options=-c%20log_statement%3Dall",
        database.connection_string
    );
    let mut previous_log_len = 0;
    for (flags, source, target) in [
        (&[][..], 0, 0),
        (&["--source-row-security-off"][..], 1, 0),
        (&["--target-row-security-off"][..], 0, 1),
        (
            &["--source-row-security-off", "--target-row-security-off"][..],
            1,
            1,
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_pgex"))
            .args([
                "--source",
                &connection_string,
                "--target",
                &connection_string,
            ])
            .arg("--ca")
            .arg(database.ca.path())
            .arg("--target-ca")
            .arg(database.ca.path())
            .env("PGEX_SOURCE_PASSWORD", &database.password)
            .env("PGEX_TARGET_PASSWORD", &database.password)
            .args(["--tables", "public.events", "--where", "company_id = 1"])
            .args(flags)
            .output()
            .await?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{flags:?}: {stderr}");

        let log = database.stderr().await?;
        let statements = &log[previous_log_len..];
        let source_sets =
            statement_backends(statements, |sql| sql == "SET LOCAL row_security = off");
        let target_sets = statement_backends(statements, |sql| sql == "SET row_security = off");
        assert_eq!(source_sets.len(), source, "{flags:?}: {statements}");
        assert_eq!(target_sets.len(), target, "{flags:?}: {statements}");
        previous_log_len = log.len();
    }
    Ok(())
}

fn statement_backends(log: &str, matches: impl Fn(&str) -> bool) -> Vec<&str> {
    log.lines()
        .filter_map(|line| {
            let (prefix, statement) = line.split_once("statement: ")?;
            if !matches(statement) {
                return None;
            }
            let (_, pid) = prefix.rsplit_once('[')?;
            let (pid, _) = pid.split_once(']')?;
            Some(pid)
        })
        .collect()
}
