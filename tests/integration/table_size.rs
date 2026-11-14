use anyhow::Result;
use pgex::{
    catalog::table_size::{TableSizeEstimator, TenantScope},
    table::QualifiedTable,
};

use super::postgres::PostgresTestDatabase;

#[tokio::test]
async fn estimates_binary_copy_from_statistics_and_reports_unknown_sizes() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let connector = database.connector().await?;
    let connection = connector.connect().await?;
    connection
        .client()
        .batch_execute(
            "CREATE TABLE public.events (
                 tenant_id integer,
                 payload bigint,
                 derived bigint GENERATED ALWAYS AS (payload + 1) STORED
             ) WITH (autovacuum_enabled = false);
             INSERT INTO public.events (tenant_id, payload)
                 SELECT CASE WHEN n <= 48 THEN 10 WHEN n <= 64 THEN 20 END, n
                 FROM generate_series(1, 128) AS n;
             ANALYZE public.events;
             CREATE TABLE public.unanalyzed (tenant_id integer) WITH (autovacuum_enabled = false);
             INSERT INTO public.unanalyzed VALUES (10);
             CREATE TABLE public.text_tenants (tenant_id text);
             INSERT INTO public.text_tenants VALUES ('not an integer');
             ANALYZE public.text_tenants;",
        )
        .await?;
    connection.close().await;
    let estimator = TableSizeEstimator::new(connector);
    let events: QualifiedTable = "public.events".parse()?;
    let unanalyzed: QualifiedTable = "public.unanalyzed".parse()?;
    let missing: QualifiedTable = "public.missing".parse()?;
    let text_tenants: QualifiedTable = "public.text_tenants".parse()?;
    let tables = [
        events.clone(),
        unanalyzed.clone(),
        missing.clone(),
        text_tenants.clone(),
    ];

    let all = estimator
        .estimate_selected_sizes(&tables, TenantScope::All)
        .await?;
    let selected = estimator
        .estimate_selected_sizes(
            &tables,
            TenantScope::Selected {
                column: "tenant_id",
                ids: &[10, 10],
            },
        )
        .await?;

    let all_events = all[&events].unwrap();
    let selected_events = selected[&events].unwrap();
    assert_eq!(all_events.estimated_rows, 128.0);
    assert_eq!(all_events.estimated_bytes, 21 + 128 * (2 + 2 * 4 + 2 + 8));
    assert_eq!(selected_events.estimated_rows, 48.0);
    assert_eq!(
        selected_events.estimated_bytes,
        21 + 48 * (2 + 2 * 4 + 2 + 8)
    );
    assert_eq!(all[&unanalyzed], None);
    assert_eq!(selected[&unanalyzed], None);
    assert_eq!(all[&missing], None);
    assert_eq!(selected[&text_tenants], None);
    Ok(())
}

#[tokio::test]
async fn includes_out_of_line_toast_bytes_in_whole_table_and_tenant_estimates() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let connector = database.connector().await?;
    let connection = connector.connect().await?;
    connection
        .client()
        .batch_execute(
            "CREATE TABLE public.toasted_events (
                 tenant_id integer NOT NULL,
                 payload text NOT NULL
             ) WITH (autovacuum_enabled = false);
             ALTER TABLE public.toasted_events ALTER COLUMN payload SET STORAGE EXTERNAL;
             INSERT INTO public.toasted_events
                 SELECT CASE WHEN n <= 8 THEN 10 ELSE 20 END, repeat('x', 16384)
                 FROM generate_series(1, 16) AS n;
             ANALYZE public.toasted_events;",
        )
        .await?;
    let row = connection
        .client()
        .query_one(
            "SELECT pg_catalog.pg_relation_size(c.reltoastrelid), s.avg_width
             FROM pg_catalog.pg_class c
             JOIN pg_catalog.pg_stats s ON s.schemaname = 'public'
                 AND s.tablename = c.relname AND s.attname = 'payload' AND NOT s.inherited
             WHERE c.oid = 'public.toasted_events'::regclass",
            &[],
        )
        .await?;
    let toast_bytes = u64::try_from(row.try_get::<_, i64>(0)?)?;
    let payload_width = u64::try_from(row.try_get::<_, i32>(1)?)?;
    connection.close().await;
    assert!(toast_bytes >= 16 * 16384);
    assert!(payload_width < 16384);

    let estimator = TableSizeEstimator::new(connector);
    let table: QualifiedTable = "public.toasted_events".parse()?;
    let tables = std::slice::from_ref(&table);
    let all = estimator
        .estimate_selected_sizes(tables, TenantScope::All)
        .await?;
    let selected = estimator
        .estimate_selected_sizes(
            tables,
            TenantScope::Selected {
                column: "tenant_id",
                ids: &[10],
            },
        )
        .await?;

    let bytes_per_row = 2 + 2 * 4 + 4 + payload_width;
    assert_eq!(
        all[&table].unwrap().estimated_bytes,
        21 + 16 * bytes_per_row + toast_bytes
    );
    assert_eq!(selected[&table].unwrap().estimated_rows, 8.0);
    assert_eq!(
        selected[&table].unwrap().estimated_bytes,
        21 + 8 * bytes_per_row + toast_bytes / 2
    );
    Ok(())
}
