use anyhow::{Result, ensure};
use pgex::connection::Row;

use pgex::snapshot::SnapshotOptions;

use super::with_snapshot_database;

#[tokio::test]
async fn imported_snapshot_excludes_rows_committed_after_export() -> Result<()> {
    with_snapshot_database(async |snapshots, observer| {
        let tables = ["public.pgex_snapshot_visibility_probe".parse()?];
        observer
            .batch_execute("CREATE TABLE public.pgex_snapshot_visibility_probe AS SELECT 1 AS id")
            .await?;

        let result = snapshots
            .with_snapshot(
                SnapshotOptions {
                    tables: &tables,
                    ..SnapshotOptions::default()
                },
                async |readers| {
                    observer
                        .execute(
                            "INSERT INTO public.pgex_snapshot_visibility_probe VALUES (2)",
                            &[],
                        )
                        .await?;
                    let imported_count: i64 = readers
                        .with_reader(async |session| {
                            let row = session
                                .client()
                                .query_one(
                                    "SELECT count(*) FROM public.pgex_snapshot_visibility_probe",
                                    &[],
                                )
                                .await?;
                            Ok::<_, pgex::connection::Error>(row.get(0))
                        })
                        .await?;
                    let current_count: i64 = observer
                        .query_one(
                            "SELECT count(*) FROM public.pgex_snapshot_visibility_probe",
                            &[],
                        )
                        .await?
                        .get(0);
                    ensure!(imported_count == 1, "The reader must use the exported view");
                    ensure!(current_count == 2, "The observer must see the newer row");
                    Ok::<_, anyhow::Error>(())
                },
            )
            .await;

        let cleanup = observer
            .batch_execute("DROP TABLE public.pgex_snapshot_visibility_probe")
            .await;
        result?;
        cleanup?;
        Ok(())
    })
    .await
}
