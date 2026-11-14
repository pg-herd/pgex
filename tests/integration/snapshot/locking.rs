use anyhow::{Result, ensure};
use tokio_postgres::{Client, error::SqlState};

use pgex::snapshot::SnapshotOptions;

use super::with_snapshot_database;

#[tokio::test]
async fn selected_table_stays_locked_until_the_snapshot_scope_ends() -> Result<()> {
    with_snapshot_database(async |snapshots, observer| {
        observer
            .batch_execute("CREATE TABLE public.snapshot_lock_probe (id bigint)")
            .await?;
        let tables = ["public.snapshot_lock_probe".parse()?];

        snapshots.with_snapshot(SnapshotOptions { tables: &tables, ..SnapshotOptions::default() },
                async |_| {
                    let lock = try_exclusive_lock(observer).await;
                    ensure!(
                        matches!(lock, Err(ref error) if error.code() == Some(&SqlState::LOCK_NOT_AVAILABLE)),
                        "expected the exporter to prevent an exclusive lock before opening a reader, got {lock:?}"
                    );
                    Ok::<_, anyhow::Error>(())
                },
            )
            .await?;

        try_exclusive_lock(observer).await?;
        Ok(())
    })
    .await
}

async fn try_exclusive_lock(observer: &Client) -> Result<(), tokio_postgres::Error> {
    observer.batch_execute("BEGIN").await?;
    let lock = observer
        .batch_execute("LOCK TABLE public.snapshot_lock_probe IN ACCESS EXCLUSIVE MODE NOWAIT")
        .await;
    observer.batch_execute("ROLLBACK").await?;
    lock
}
