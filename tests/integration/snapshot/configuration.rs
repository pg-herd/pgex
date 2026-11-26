use crate::postgres::sql_state;
use pgex::connection::Row;
use std::time::Duration;

use anyhow::{Context, Result};
use pgex::connection::Client;

use pgex::snapshot::{
    SnapshotOptions, SnapshotScopeError, SnapshotTimeouts, SourceReadMode::SharedSnapshot,
};

use super::with_snapshot_database;

const TRANSACTION_TIMEOUT_SQLSTATE: &str = "25P04";

#[tokio::test]
async fn disconnect_check_interval_override_is_limited_to_snapshot_work() -> Result<()> {
    with_snapshot_database(async |snapshots, observer| {
        assert_eq!(connection_check_interval(observer).await?, 0);

        snapshots
            .with_snapshot(
                SnapshotOptions {
                    client_connection_check_interval_ms: 250,
                    ..SnapshotOptions::default()
                },
                SharedSnapshot,
                async |readers| {
                    let interval = readers
                        .with_reader(async |session| {
                            connection_check_interval(session.client()).await
                        })
                        .await?;
                    assert_eq!(interval, 250);
                    assert_eq!(connection_check_interval(observer).await?, 0);
                    Ok::<_, anyhow::Error>(())
                },
            )
            .await?;

        assert_eq!(connection_check_interval(observer).await?, 0);
        Ok(())
    })
    .await
}

async fn connection_check_interval(client: &Client) -> Result<i32, pgex::connection::Error> {
    let row = client
        .query_one(
            "SELECT setting::integer FROM pg_settings WHERE name = 'client_connection_check_interval'",
            &[],
        )
        .await?;
    row.try_get(0)
}

#[tokio::test]
async fn readers_disable_inherited_timeouts_and_apply_explicit_overrides() -> Result<()> {
    with_snapshot_database(async |snapshots, observer| {
        observer
            .batch_execute(
                "ALTER ROLE CURRENT_USER SET transaction_timeout = '30s';
                 ALTER ROLE CURRENT_USER SET idle_in_transaction_session_timeout = '20s'",
            )
            .await?;

        for (transaction_ms, idle_in_transaction_ms, expected) in [
            (0, 0, [0, 0]),
            (15_000, 0, [15_000, 0]),
            (0, 10_000, [0, 10_000]),
        ] {
            let options = SnapshotOptions {
                exporter_timeouts: SnapshotTimeouts {
                    transaction_ms: 45_000,
                    idle_in_transaction_ms: 40_000,
                },
                reader_timeouts: SnapshotTimeouts {
                    transaction_ms,
                    idle_in_transaction_ms,
                },
                ..SnapshotOptions::default()
            };
            let actual = snapshots
                .with_snapshot(options, SharedSnapshot, async |readers| {
                    readers
                        .with_reader(async |session| read_timeouts(session.client()).await)
                        .await
                })
                .await?;
            assert_eq!(actual, expected);
        }
        Ok(())
    })
    .await
}

async fn read_timeouts(client: &Client) -> Result<[i32; 2], pgex::connection::Error> {
    let rows = client
        .query(
            "SELECT setting::integer FROM pg_settings
             WHERE name IN ('transaction_timeout', 'idle_in_transaction_session_timeout')
             ORDER BY name DESC",
            &[],
        )
        .await?;
    Ok([rows[0].try_get(0)?, rows[1].try_get(0)?])
}

#[tokio::test]
async fn reader_transaction_timeout_replaces_inherited_timer_and_stops_active_work() -> Result<()> {
    with_snapshot_database(async |snapshots, observer| {
        observer
            .batch_execute("ALTER ROLE CURRENT_USER SET transaction_timeout = '30s'")
            .await?;
        snapshots
            .with_snapshot(
                SnapshotOptions {
                    reader_timeouts: SnapshotTimeouts {
                        transaction_ms: 750,
                        ..SnapshotTimeouts::default()
                    },
                    ..SnapshotOptions::default()
                },
                SharedSnapshot,
                async |readers| {
                    let reader = readers.with_reader(async |session| {
                        session.client().batch_execute("SELECT pg_sleep(10)").await
                    });
                    let result = tokio::time::timeout(Duration::from_secs(5), reader).await?;
                    let Err(SnapshotScopeError::Rollback {
                        prior_failure: Some(failure),
                        ..
                    }) = result
                    else {
                        anyhow::bail!(
                            "Expected the reader transaction to time out, got {result:?}"
                        );
                    };
                    assert!(
                        matches!(failure.as_ref(), SnapshotScopeError::Work(error)
                if sql_state(error).as_deref() == Some(TRANSACTION_TIMEOUT_SQLSTATE)),
                        "{failure:?}"
                    );

                    readers
                        .with_reader(async |session| {
                            session.client().batch_execute("SELECT 1").await
                        })
                        .await?;
                    Ok::<_, anyhow::Error>(())
                },
            )
            .await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn exporter_timeouts_release_its_session_while_work_is_pending() -> Result<()> {
    with_snapshot_database(async |snapshots, _| {
        for (transaction_ms, idle_in_transaction_ms, expected_code) in
            [(750, 0, TRANSACTION_TIMEOUT_SQLSTATE), (0, 350, "25P03")]
        {
            let exporter_timeouts = SnapshotTimeouts {
                transaction_ms,
                idle_in_transaction_ms,
            };
            let scope = snapshots.with_snapshot(
                SnapshotOptions {
                    exporter_timeouts,
                    ..SnapshotOptions::default()
                },
                SharedSnapshot,
                async |_| std::future::pending::<Result<()>>().await,
            );
            let result = tokio::time::timeout(Duration::from_secs(5), scope)
                .await
                .context("Snapshot scope kept running after the exporter timeout")?;
            assert!(
                matches!(&result, Err(SnapshotScopeError::ExporterLost(error))
                    if error.downcast_ref::<pgex::connection::Error>()
                        .and_then(sql_state).as_deref()
                        .is_some_and(|code| code == expected_code)),
                "{exporter_timeouts:?}: {result:?}"
            );
        }
        Ok(())
    })
    .await
}
