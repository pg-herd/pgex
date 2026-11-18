use crate::postgres::sql_state;
use pgex::connection::Row;
use std::{convert::Infallible, future::pending, time::Duration};

use anyhow::{Context, Result, ensure};
use pgex::connection::Client;
use tokio::sync::oneshot;

use pgex::{
    connection::Connector,
    snapshot::{SnapshotLease, SnapshotOptions, SnapshotScopeError, SnapshotTimeouts},
};

use super::{PostgresTestDatabase, with_snapshot_database};

#[tokio::test]
async fn closes_snapshot_sessions_after_success() -> Result<()> {
    with_snapshot_database(async |snapshots, observer| {
        snapshots
            .with_snapshot(SnapshotOptions::default(), async |readers| {
                readers
                    .with_reader(async |session| {
                        session.client().query_one("SELECT 1", &[]).await?;
                        Ok::<_, pgex::connection::Error>(())
                    })
                    .await
            })
            .await?;
        assert_sessions_closed(observer).await
    })
    .await
}

#[tokio::test]
async fn closes_snapshot_sessions_after_callback_failure() -> Result<()> {
    const CALLBACK_FAILURE: &str = "intentional callback failure";

    with_snapshot_database(async |snapshots, observer| {
        let result = snapshots
            .with_snapshot(SnapshotOptions::default(), async |readers| {
                readers
                    .with_reader(async |session| {
                        session.client().query_one("SELECT 1", &[]).await?;
                        Err::<(), _>(anyhow::anyhow!(CALLBACK_FAILURE))
                    })
                    .await
            })
            .await;
        ensure!(
            matches!(
                result,
                Err(SnapshotScopeError::Work(SnapshotScopeError::Work(ref error)))
                    if error.to_string() == CALLBACK_FAILURE
            ),
            "Expected a callback failure, got {result:?}"
        );
        assert_sessions_closed(observer).await
    })
    .await
}

#[tokio::test]
async fn closes_exporting_session_when_postgresql_rejects_the_lock() -> Result<()> {
    with_snapshot_database(async |snapshots, observer| {
        let tables = [
            "public.pgex_snapshot_missing_lock_target".parse()?,
            "public.another_missing_table".parse()?,
        ];
        let result = snapshots
            .with_snapshot(
                SnapshotOptions {
                    tables: &tables,
                    ..SnapshotOptions::default()
                },
                async |_| -> Result<(), Infallible> {
                    panic!("the callback ran after PostgreSQL rejected the lock");
                },
            )
            .await;

        ensure!(
            matches!(result, Err(SnapshotScopeError::Setup(ref error))
                if error.to_string() == r#"Table "public"."pgex_snapshot_missing_lock_target" does not exist"#
                    && error.downcast_ref::<pgex::connection::Error>().and_then(sql_state).as_deref()
                    == Some("42P01")),
            "expected a missing table error, got {result:?}"
        );
        assert_sessions_closed(observer).await
    })
    .await
}

#[tokio::test]
async fn closes_exporting_session_when_transaction_setup_fails() -> Result<()> {
    with_snapshot_database(async |snapshots, observer| {
        let result = snapshots
            .with_snapshot(
                SnapshotOptions {
                    client_connection_check_interval_ms: u32::MAX,
                    ..SnapshotOptions::default()
                },
                async |_| -> Result<(), Infallible> {
                    panic!("the callback ran after transaction setup failed");
                },
            )
            .await;

        ensure!(matches!(result, Err(SnapshotScopeError::Setup(ref error))
            if error.to_string() == "client_connection_check_interval must fit in a PostgreSQL integer"),
            "expected an interval validation error, got {result:?}");
        assert_sessions_closed(observer).await
    })
    .await
}

#[tokio::test]
async fn closes_exporting_session_when_postgresql_rejects_snapshot_export() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let observer = database.connector().await?.connect().await?;
    observer
        .client()
        .batch_execute(
            "CREATE ROLE snapshot_reader;
             REVOKE EXECUTE ON FUNCTION pg_catalog.pg_export_snapshot() FROM PUBLIC;",
        )
        .await?;
    let connection_string = format!(
        "{}?options=-c%20role%3Dsnapshot_reader",
        database.connection_string
    );
    let source = Connector::new(
        &connection_string,
        Some(database.ca.path()),
        Some(&database.password),
    )
    .await?;
    let result = SnapshotLease::new(source)
        .with_snapshot(
            SnapshotOptions::default(),
            async |_| -> Result<(), Infallible> {
                panic!("the callback ran after PostgreSQL rejected snapshot export");
            },
        )
        .await;

    ensure!(
        matches!(result, Err(SnapshotScopeError::Setup(ref error))
            if error.downcast_ref::<pgex::connection::Error>().and_then(sql_state).as_deref()
                == Some("42501")),
        "expected a snapshot export permission error, got {result:?}"
    );
    assert_sessions_closed(observer.client()).await?;
    observer.close().await;
    Ok(())
}

#[tokio::test]
async fn exporter_disconnect_stops_the_snapshot_callback() -> Result<()> {
    with_snapshot_database(async |snapshots, observer| {
        let result = snapshots
            .with_snapshot(SnapshotOptions::default(), async |_| {
                // The observer and the exporter are the only sessions in this database.
                let terminated: bool = observer
                    .query_one(
                        "SELECT pg_terminate_backend(pid, 5000)
                             FROM pg_stat_activity
                             WHERE datname = current_database()
                               AND backend_type = 'client backend'
                               AND pid <> pg_backend_pid()",
                        &[],
                    )
                    .await?
                    .get(0);
                ensure!(terminated, "could not terminate the exporting session");

                pending::<Result<()>>().await
            })
            .await;

        ensure!(
            matches!(result, Err(SnapshotScopeError::ExporterLost(_))),
            "expected loss of the exporter, got {result:?}"
        );
        assert_sessions_closed(observer).await
    })
    .await
}

#[tokio::test]
async fn exporter_timeout_closes_worker_sessions_and_rolls_back_destination_write() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let connector = database.connector().await?;
    let observer_session = connector.connect().await?;
    let observer = observer_session.client();
    observer
        .batch_execute(
            "CREATE TABLE public.exporter_loss_probe (payload text);
             INSERT INTO public.exporter_loss_probe VALUES ('original')",
        )
        .await?;
    let tables = ["public.exporter_loss_probe".parse()?];
    let snapshots = SnapshotLease::new(connector.clone());
    let (write_finished, mut write_ready) = oneshot::channel();
    let scope = snapshots.with_snapshot(
        SnapshotOptions {
            tables: &tables,
            exporter_timeouts: SnapshotTimeouts {
                idle_in_transaction_ms: 1_500,
                ..SnapshotTimeouts::default()
            },
            ..SnapshotOptions::default()
        },
        async move |readers| {
            readers
                .with_reader(async move |source| {
                    source.client().query_one("SELECT 1", &[]).await?;
                    connector
                        .with_connection(async move |destination| {
                            destination
                                .with_transaction(async move |destination| {
                                    let changed = destination.client().execute(
                            "UPDATE public.exporter_loss_probe SET payload = 'replacement'", &[],
                        ).await?;
                                    ensure!(changed == 1, "Expected the worker to update one row");
                                    let _ = write_finished.send(());
                                    pending::<Result<()>>().await
                                })
                                .await
                        })
                        .await
                })
                .await
        },
    );

    let result = tokio::time::timeout(Duration::from_secs(5), scope)
        .await
        .context("The snapshot scope kept running after the exporter timed out")?;
    write_ready
        .try_recv()
        .context("The worker did not complete the write before the timeout")?;
    assert!(
        matches!(&result, Err(SnapshotScopeError::ExporterLost(error))
        if error.downcast_ref::<pgex::connection::Error>().and_then(sql_state).as_deref()
            == Some("25P03")),
        "{result:?}"
    );
    assert_sessions_closed(observer).await?;
    let row = observer
        .query_one("SELECT payload FROM public.exporter_loss_probe", &[])
        .await?;
    assert_eq!(row.get::<&str, _>(0), "original");
    observer_session.close().await;
    Ok(())
}

#[tokio::test]
async fn preserves_work_error_when_reader_rollback_fails() -> Result<()> {
    const WORK_FAILURE: &str = "reader work failed";

    with_snapshot_database(async |snapshots, observer| {
        snapshots
            .with_snapshot(SnapshotOptions::default(), async |readers| {
                let result: Result<(), _> = readers
                    .with_reader(async |session| {
                        let pid: i32 = session
                            .client()
                            .query_one("SELECT pg_backend_pid()", &[])
                            .await?
                            .try_get(0)?;
                        let terminated: bool = observer
                            .query_one("SELECT pg_terminate_backend($1, 5000)", &[&pid])
                            .await?
                            .try_get(0)?;
                        ensure!(terminated, "could not terminate the reader session");
                        anyhow::bail!(WORK_FAILURE);
                    })
                    .await;

                let Err(SnapshotScopeError::Rollback {
                    error,
                    prior_failure: Some(failure),
                }) = result
                else {
                    anyhow::bail!("expected both work and rollback errors, got {result:?}");
                };
                ensure!(
                    matches!(*failure, SnapshotScopeError::Work(ref error)
                    if error.to_string() == WORK_FAILURE),
                    "expected the original work error"
                );
                assert_eq!(error.to_string(), "Roll back source reader");
                Ok::<_, anyhow::Error>(())
            })
            .await?;
        assert_sessions_closed(observer).await
    })
    .await
}

#[tokio::test]
async fn closes_exporting_and_read_sessions_when_the_scope_is_cancelled() -> Result<()> {
    with_snapshot_database(async |snapshots, observer| {
        let (reader_started, started) = oneshot::channel();
        let mut scope = Box::pin(snapshots.with_snapshot(SnapshotOptions::default(),
            async move |readers| {
                readers.with_reader(async move |_| {
                        reader_started.send(()).unwrap();
                        pending::<Result<()>>().await
                    })
                    .await
            },
        ));

        tokio::select! {
            result = &mut scope => anyhow::bail!("the snapshot scope ended before cancellation: {result:?}"),
            ready = tokio::time::timeout(Duration::from_secs(5), started) => {
                ready.context("the reader did not import the snapshot")??;
            }
        }

        drop(scope);
        assert_sessions_closed(observer).await
    })
    .await
}

#[tokio::test]
async fn closes_snapshot_sessions_when_cancelled_during_an_active_query() -> Result<()> {
    const LONG_QUERY: &str = "SELECT pg_sleep(30) /* snapshot cancellation probe */";

    with_snapshot_database(async |snapshots, observer| {
        let mut scope = Box::pin(snapshots.with_snapshot(SnapshotOptions::default(),
            async |readers| {
                readers.with_reader(async |session| {
                        session.client().query_one(LONG_QUERY, &[]).await.map(|_| ())
                    })
                    .await
            },
        ));

        tokio::select! {
            result = &mut scope => anyhow::bail!("the snapshot scope ended before cancellation: {result:?}"),
            active = wait_for_active_query(observer, LONG_QUERY) => active?,
        }

        drop(scope);
        assert_sessions_closed(observer).await
    })
    .await
}

async fn wait_for_active_query(observer: &Client, query: &str) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let active: bool = observer
                .query_one(
                    "SELECT EXISTS (
                         SELECT FROM pg_stat_activity
                         WHERE datname = current_database()
                           AND pid <> pg_backend_pid()
                           AND state = 'active'
                           AND query = $1
                     )",
                    &[&query],
                )
                .await?
                .get(0);
            if active {
                return Ok::<_, pgex::connection::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .context("PostgreSQL did not start the cancellation probe query")??;
    Ok(())
}

async fn assert_sessions_closed(observer: &Client) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let sessions: i64 = observer
                .query_one(
                    "SELECT count(*) FROM pg_stat_activity
                     WHERE datname = current_database()
                       AND backend_type = 'client backend'
                       AND pid <> pg_backend_pid()",
                    &[],
                )
                .await?
                .get(0);
            if sessions == 0 {
                return Ok::<(), pgex::connection::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .context("Snapshot sessions stayed open after the callback finished")??;
    Ok(())
}
