use pgex::connection::Row;
use std::{future::poll_fn, num::NonZeroUsize, task::Poll};

use anyhow::{Result, ensure};
use pgex::snapshot::{SnapshotOptions, SourceReadMode::SharedSnapshot};
use tokio::sync::oneshot;

use super::with_snapshot_database;

#[tokio::test]
async fn waits_for_a_source_reader_slot_and_releases_it_after_cancellation() -> Result<()> {
    with_snapshot_database(async |snapshots, observer| {
        snapshots
            .with_snapshot(
                SnapshotOptions {
                    reader_connection_limit: NonZeroUsize::MIN,
                    ..SnapshotOptions::default()
                },
                SharedSnapshot,
                async |readers| {
                    let (started, ready) = oneshot::channel();
                    let mut first = Box::pin(readers.with_reader(async move |_| {
                        started.send(()).unwrap();
                        std::future::pending::<Result<()>>().await
                    }));
                    tokio::select! {
                        result = &mut first => anyhow::bail!("first reader returned: {result:?}"),
                        ready = ready => ready?,
                    }

                    let mut second = Box::pin(readers.with_reader(async |session| {
                        let row = session.client().query_one("SELECT 1", &[]).await?;
                        row.try_get::<i32, _>(0)
                    }));
                    poll_fn(|context| {
                        assert!(second.as_mut().poll(context).is_pending());
                        Poll::Ready(())
                    })
                    .await;

                    let sessions: i64 = observer
                        .query_one(
                            "SELECT count(*) FROM pg_stat_activity
                             WHERE datname = current_database()
                               AND backend_type = 'client backend'
                               AND pid <> pg_backend_pid()",
                            &[],
                        )
                        .await?
                        .try_get(0)?;
                    ensure!(
                        sessions == 2,
                        "expected one exporter and one reader, got {sessions}"
                    );

                    drop(first);
                    assert_eq!(second.await?, 1);
                    Ok::<_, anyhow::Error>(())
                },
            )
            .await?;
        Ok(())
    })
    .await
}
