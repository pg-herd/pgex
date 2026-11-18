use crate::postgres::sql_state;
use anyhow::{Result, bail};
use pgex::connection::Row;
use pgex::table::with_locked_tables;

use super::postgres::PostgresTestDatabase;

#[tokio::test]
async fn table_locks_survive_worker_commits_and_release_on_callback_failure() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let connector = database.connector().await?;
    let mut worker = connector.connect().await?;
    worker
        .client()
        .batch_execute("CREATE TABLE public.lock_probe (id integer); SET lock_timeout = '250ms'")
        .await?;
    let tables = ["public.lock_probe".parse()?];

    let result: Result<()> = with_locked_tables(&connector, &tables, async |_| {
        for id in [1_i32, 2] {
            worker
                .with_transaction(async |session| {
                    session
                        .client()
                        .execute("INSERT INTO public.lock_probe VALUES ($1)", &[&id])
                        .await?;
                    Ok(())
                })
                .await?;
        }
        let alter = worker
            .client()
            .batch_execute("ALTER TABLE public.lock_probe ADD COLUMN value text")
            .await;
        assert!(matches!(alter, Err(error) if sql_state(&error).as_deref() == Some("55P03")));
        bail!("abort lock scope")
    })
    .await;

    assert_eq!(result.unwrap_err().to_string(), "abort lock scope");
    worker
        .client()
        .batch_execute("ALTER TABLE public.lock_probe ADD COLUMN value text")
        .await?;
    let count: i64 = worker
        .client()
        .query_one("SELECT count(*) FROM public.lock_probe", &[])
        .await?
        .try_get(0)?;
    worker.close().await;
    assert_eq!(count, 2);
    Ok(())
}
