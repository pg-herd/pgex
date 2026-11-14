use anyhow::Result;
use pgex::{catalog::CatalogReader, table::QualifiedTable};

use super::postgres::PostgresTestDatabase;

#[tokio::test]
async fn discovers_table_names_across_non_system_schemas() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let connector = database.connector().await?;
    let connection = connector.connect().await?;
    connection
        .client()
        .batch_execute(
            r#"
            CREATE SCHEMA selected;
            CREATE TABLE selected.events (id integer) PARTITION BY RANGE (id);
            CREATE TABLE selected.events_part PARTITION OF selected.events FOR VALUES FROM (0) TO (10);
            CREATE TABLE selected."with""quote" (id integer);
            CREATE VIEW selected.events_view AS SELECT * FROM selected.events;
            CREATE TABLE public.events (id integer);
            "#,
        )
        .await?;
    connection.close().await;
    let reader = CatalogReader::new(connector);

    let names = reader.list_tables().await?;

    assert_eq!(
        names
            .iter()
            .map(|name| (name.schema(), name.table()))
            .collect::<Vec<_>>(),
        [
            ("public", "events"),
            ("selected", "events"),
            ("selected", r#"with"quote"#),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn reads_ordered_layouts_and_enum_labels_on_the_locked_connection() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let connector = database.connector().await?;
    let connection = connector.connect().await?;
    let reader = CatalogReader::new(connector);
    connection
        .client()
        .batch_execute(
            "BEGIN;
             CREATE TYPE public.priority AS ENUM ('low', 'high');
             ALTER TYPE public.priority ADD VALUE 'medium' BEFORE 'high';
             CREATE TABLE public.events (
                 priority public.priority NOT NULL,
                 removed_column integer,
                 title varchar(16),
                 title_len integer GENERATED ALWAYS AS (length(title)) STORED
             );
             ALTER TABLE public.events DROP COLUMN removed_column;
             CREATE TABLE public.empty_layout ();
             LOCK TABLE public.events, public.empty_layout IN ACCESS SHARE MODE;",
        )
        .await?;

    let events: QualifiedTable = "public.events".parse()?;
    let empty: QualifiedTable = "public.empty_layout".parse()?;
    let catalog = reader
        .read_tables_on(
            &connection,
            &[events.clone(), empty.clone(), events.clone()],
        )
        .await?;
    let missing_table = "public.missing_layout".parse()?;
    let missing_error = reader
        .read_tables_on(
            &connection,
            &[events.clone(), missing_table, events.clone()],
        )
        .await
        .unwrap_err();
    connection.client().batch_execute("ROLLBACK").await?;
    connection.close().await;

    assert_eq!(
        missing_error.to_string(),
        r#"No table layout found for "public"."missing_layout""#
    );
    assert_eq!(catalog.tables.len(), 2);
    assert_eq!(catalog.tables[0].name, empty);
    assert!(catalog.tables[0].columns.is_empty());
    let table = &catalog.tables[1];
    assert_eq!(table.name, events);
    assert!(table.relation_oid > 0);
    let columns = &table.columns;
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["priority", "title", "title_len"]
    );
    assert_eq!(columns[0].data_type.schema, "public");
    assert_eq!(columns[0].data_type.name, "priority");
    assert_eq!(
        columns[0].data_type.enum_labels,
        Some(vec!["low".into(), "medium".into(), "high".into()])
    );
    assert!(!columns[0].nullable);
    assert!(columns[1].nullable);
    assert_eq!(columns[1].data_type.name, "varchar");
    assert_eq!(columns[1].data_type.modifier, 20);
    assert_eq!(columns[1].data_type.enum_labels, None);
    assert!(columns[2].generated);
    Ok(())
}
