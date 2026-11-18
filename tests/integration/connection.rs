use pgex::connection::Row;
use std::future::pending;

use anyhow::{Result, bail};
use tokio::sync::oneshot;

use super::postgres::PostgresTestDatabase;

#[tokio::test]
async fn drains_large_tls_results_without_losing_readiness() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let session = database.connector().await?.connect().await?;
    let rows = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        session.client().query(
            "SELECT n, repeat('x', 512) AS payload FROM generate_series(1, 20000) AS n",
            &[],
        ),
    )
    .await??;
    assert_eq!(rows.len(), 20000);
    assert_eq!(rows.last().unwrap().get::<i32, _>(0), 20000);
    assert_eq!(rows[0].get::<String, _>(1).len(), 512);
    session.close().await;
    Ok(())
}

#[tokio::test]
async fn reuses_one_connection_after_commit_and_rollback() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let connector = database.connector().await?;

    connector
        .with_connection(async |connection| {
            let original_pid: i32 = connection
                .with_transaction(async |session| {
                    session
                        .client().batch_execute(
                            "CREATE TEMP TABLE transaction_probe (id integer);
                             INSERT INTO transaction_probe VALUES (1);",
                        )
                        .await?;
                    Ok(session.client().query_one("SELECT pg_backend_pid()", &[]).await?.try_get(0)?)
                })
                .await?;

            let failure: Result<()> = connection
                .with_transaction(async |session| {
                    session
                        .client().batch_execute("INSERT INTO transaction_probe VALUES (2)")
                        .await?;
                    bail!("abort this transaction")
                })
                .await;
            assert_eq!(failure.unwrap_err().to_string(), "abort this transaction");

            connection
                .with_transaction(async |session| {
                    let row = session
                        .client().query_one(
                            "SELECT pg_backend_pid(), array_agg(id ORDER BY id) FROM transaction_probe",
                            &[],
                        )
                        .await?;
                    assert_eq!(row.try_get::<i32, _>(0)?, original_pid);
                    assert_eq!(row.try_get::<Vec<i32>, _>(1)?, [1]);
                    session
                        .client().batch_execute("INSERT INTO transaction_probe VALUES (3)")
                        .await?;
                    Ok(())
                })
                .await?;

            let row = connection
                .client()
                .query_one("SELECT array_agg(id ORDER BY id) FROM transaction_probe", &[])
                .await?;
            assert_eq!(row.try_get::<Vec<i32>, _>(0)?, [1, 3]);
            Ok(())
        })
        .await
}

#[tokio::test]
async fn cancelling_transaction_work_closes_the_session() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let connector = database.connector().await?;

    connector
        .with_connection(async |session| {
            let (started, ready) = oneshot::channel();
            let mut work = Box::pin(session.with_transaction(async move |session| {
                session.client().query_one("SELECT 1", &[]).await?;
                started.send(()).unwrap();
                pending::<Result<()>>().await
            }));

            tokio::select! {
                result = &mut work => bail!("transaction ended before cancellation: {result:?}"),
                ready = ready => ready?,
            }
            drop(work);

            assert!(session.client().query_one("SELECT 1", &[]).await.is_err());
            Ok(())
        })
        .await
}

#[tokio::test]
async fn tries_the_next_host_and_honors_target_session_attributes() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let url = url::Url::parse(&database.connection_string)?;
    let host = url.host_str().unwrap();
    let port = url.port().unwrap();
    let connection_string = format!(
        "postgresql://pgex_test@127.0.0.1:1,{host}:{port}/pgex_snapshot_test?connect_timeout=1&target_session_attrs=read-write"
    );
    let connector = pgex::connection::Connector::new(
        &connection_string,
        Some(database.ca.path()),
        Some(&database.password),
    )
    .await?;
    let session = connector.connect().await?;
    let value: i32 = session
        .client()
        .query_one("SELECT 42", &[])
        .await?
        .try_get(0)?;
    assert_eq!(value, 42);
    session.close().await;
    Ok(())
}

#[tokio::test]
async fn requires_a_trusted_certificate_even_when_url_disables_tls() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let connection_string = format!("{}?sslmode=disable", database.connection_string);
    let untrusted =
        pgex::connection::Connector::new(&connection_string, None, Some(&database.password))
            .await?;
    let error = untrusted
        .connect()
        .await
        .err()
        .expect("An untrusted certificate must fail");
    assert!(
        format!("{error:#}").to_lowercase().contains("certificate"),
        "{error:#}"
    );
    let trusted = pgex::connection::Connector::new(
        &connection_string,
        Some(database.ca.path()),
        Some(&database.password),
    )
    .await?;
    trusted.connect().await?.close().await;
    Ok(())
}

#[tokio::test]
async fn libpq_quoting_round_trips_literals_and_identifiers() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let session = database.connector().await?.connect().await?;
    let client = session.client();
    let relation = client
        .quote_qualified_identifier("public", "Mixed \"雪\"; DROP TABLE nope")
        .await?;
    let column = {
        let mut guard = client.connection().await?;
        guard.get().await?.quote_identifier("va\"lue")?
    };
    client
        .batch_execute(&format!("CREATE TABLE {relation} ({column} text)"))
        .await?;
    for setting in ["on", "off"] {
        client
            .batch_execute(&format!("SET standard_conforming_strings = {setting}"))
            .await?;
        for value in [
            "",
            "O'Reilly\\notes",
            "café 雪\n\t\r",
            "'; DROP TABLE nope; --",
        ] {
            let literal = client.quote_literal(value).await?;
            client
                .batch_execute(&format!("INSERT INTO {relation} VALUES ({literal})"))
                .await?;
            let row = client
                .query_one(&format!("SELECT {literal}::text"), &[])
                .await?;
            assert_eq!(row.get::<String, _>(0), value);
        }
    }
    let count: i64 = client
        .query_one(&format!("SELECT count(*) FROM {relation}"), &[])
        .await?
        .try_get(0)?;
    assert_eq!(count, 8);
    assert!(client.quote_literal("before\0after").await.is_err());
    assert!(
        client
            .quote_qualified_identifier("public", "before\0after")
            .await
            .is_err()
    );
    session.close().await;
    Ok(())
}

#[tokio::test]
async fn copy_reader_preserves_chunks_eof_errors_and_connection_reuse() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let session = database.connector().await?.connect().await?;
    let mut guard = session.client().connection().await?;
    let connection = guard.get().await?;
    let retained = {
        let mut copy = connection
            .copy_out_raw("COPY (SELECT n FROM generate_series(1, 3) n) TO STDOUT")
            .await?;
        let first = copy.next_chunk().await?.unwrap();
        let mut output = first.to_vec();
        while let Some(chunk) = copy.next_chunk().await? {
            output.extend_from_slice(&chunk);
        }
        assert_eq!(output, b"1\n2\n3\n");
        assert!(copy.next_chunk().await?.is_none());
        first
    };
    {
        let mut copy = connection
            .copy_out_raw("COPY (SELECT 1 / (n - 1) FROM generate_series(1, 2) n) TO STDOUT")
            .await?;
        let failure = loop {
            match copy.next_chunk().await {
                Err(error) => break error,
                Ok(Some(_)) => {}
                Ok(None) => bail!("The division by zero must fail COPY"),
            }
        };
        assert_eq!(failure.code().as_deref(), Some("22012"));
    }
    drop(guard);
    assert_eq!(
        session
            .client()
            .query_one("SELECT 42", &[])
            .await?
            .get::<i32, _>(0),
        42
    );
    session.close().await;
    // COPY chunks own their libpq allocation independently of the session.
    assert_eq!(&retained[..], b"1\n");
    Ok(())
}
