use crate::postgres::sql_state;
use anyhow::{Result, ensure};
use pgex::connection::Client;

use pgex::snapshot::{SnapshotOptions, SourceReadMode};

use super::with_snapshot_database;

#[tokio::test]
async fn reader_releases_table_lock_while_coordinator_is_still_open() -> Result<()> {
    with_snapshot_database(async |snapshots, observer| {
        observer
            .batch_execute("CREATE TABLE public.snapshot_lock_probe (id bigint)")
            .await?;
        for mode in [SourceReadMode::SharedSnapshot, SourceReadMode::ReadCommitted] {
            snapshots.with_snapshot(SnapshotOptions::default(), mode, async |readers| {
                try_exclusive_lock(observer).await?;
                readers.with_reader(async |session| {
                    session.client().batch_execute("SELECT * FROM public.snapshot_lock_probe").await?;
                    ensure!(matches!(try_exclusive_lock(observer).await, Err(ref error) if sql_state(error).as_deref() == Some("55P03")),
                        "the active reader must hold the relation lock");
                    anyhow::Ok(())
                }).await?;
                try_exclusive_lock(observer).await?;
                anyhow::Ok(())
            }).await?;
        }
        try_exclusive_lock(observer).await?;
        Ok(())
    })
    .await
}

async fn try_exclusive_lock(observer: &Client) -> Result<(), pgex::connection::Error> {
    observer.batch_execute("BEGIN").await?;
    let lock = observer
        .batch_execute("LOCK TABLE public.snapshot_lock_probe IN ACCESS EXCLUSIVE MODE NOWAIT")
        .await;
    observer.batch_execute("ROLLBACK").await?;
    lock
}
