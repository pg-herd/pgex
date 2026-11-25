use std::collections::HashSet;

use crate::connection::{Row, RowData};
use anyhow::{Context, Result, ensure};

use crate::{connection::Session, table::QualifiedTable};

#[derive(Debug, Eq, PartialEq)]
pub struct Catalog {
    pub tables: Vec<TableLayout>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct TableLayout {
    pub name: QualifiedTable,
    /// Local to the database. Use `has_same_columns` to compare layouts across databases.
    pub relation_oid: u32,
    pub columns: Vec<ColumnLayout>,
}

impl TableLayout {
    /// Compares ordered column metadata, including the label order of scalar enums.
    pub fn has_same_columns(&self, other: &Self) -> bool {
        self.columns == other.columns
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ColumnLayout {
    pub name: String,
    pub data_type: PgType,
    pub nullable: bool,
    pub generated: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub struct PgType {
    pub schema: String,
    pub name: String,
    /// PostgreSQL's type-specific modifier, such as a declared length or precision.
    pub modifier: i32,
    /// The labels of a scalar enum in PostgreSQL comparison order.
    pub enum_labels: Option<Vec<String>>,
}

pub(super) async fn read_tables_on(
    connection: &Session,
    tables: &[QualifiedTable],
) -> Result<Catalog> {
    if tables.is_empty() {
        return Ok(Catalog { tables: Vec::new() });
    }

    let (schemas, names): (Vec<_>, Vec<_>) = tables
        .iter()
        .map(|table| (table.schema().to_owned(), table.table().to_owned()))
        .unzip();
    let rows = connection
        .client()
        .query(include_str!("layouts.sql"), &[&schemas, &names])
        .await
        .context("Read PostgreSQL table layouts")?;

    let layout_rows = rows
        .into_iter()
        .map(LayoutRow::try_from)
        .collect::<Result<Vec<_>>>()
        .context("Decode PostgreSQL table layout rows")?;
    let layouts = group_layout_rows(layout_rows);
    let found: HashSet<_> = layouts
        .iter()
        .map(|layout| (layout.name.schema(), layout.name.table()))
        .collect();
    for table in tables {
        ensure!(
            found.contains(&(table.schema(), table.table())),
            "No table layout found for {:?}.{:?}",
            table.schema(),
            table.table()
        );
    }
    Ok(Catalog { tables: layouts })
}

struct LayoutRow {
    table_name: QualifiedTable,
    relation_oid: u32,
    column: Option<ColumnLayout>,
}

impl TryFrom<RowData> for LayoutRow {
    type Error = anyhow::Error;

    fn try_from(row: RowData) -> Result<Self> {
        Ok(Self {
            table_name: QualifiedTable::from_catalog(
                row.try_get("schema_name")?,
                row.try_get("table_name")?,
            ),
            relation_oid: row.try_get::<u32, _>("relation_oid")?,
            column: read_column(&row)?,
        })
    }
}

fn read_column(row: &RowData) -> Result<Option<ColumnLayout>> {
    let Some(name) = row.try_get("column_name")? else {
        return Ok(None);
    };

    Ok(Some(ColumnLayout {
        name,
        data_type: PgType {
            schema: row.try_get("type_schema")?,
            name: row.try_get("type_name")?,
            modifier: row.try_get("type_modifier")?,
            enum_labels: row.try_get("enum_labels")?,
        },
        nullable: row.try_get("nullable")?,
        generated: row.try_get("generated")?,
    }))
}

fn group_layout_rows(rows: Vec<LayoutRow>) -> Vec<TableLayout> {
    let mut layouts: Vec<TableLayout> = Vec::new();
    // The query orders rows so each table's columns are adjacent.
    for row in rows {
        let table = match layouts.last_mut() {
            Some(table) if table.relation_oid == row.relation_oid => table,
            _ => layouts.push_mut(TableLayout {
                name: row.table_name,
                relation_oid: row.relation_oid,
                columns: Vec::new(),
            }),
        };
        if let Some(column) = row.column {
            table.columns.push(column);
        }
    }
    layouts
}

#[cfg(test)]
mod tests {
    use super::{ColumnLayout, PgType, TableLayout};

    fn layout(relation_oid: u32, labels: [&str; 2]) -> TableLayout {
        TableLayout {
            name: "public.events".parse().unwrap(),
            relation_oid,
            columns: vec![ColumnLayout {
                name: "priority".into(),
                data_type: PgType {
                    schema: "public".into(),
                    name: "priority".into(),
                    modifier: -1,
                    enum_labels: Some(labels.map(str::to_owned).into()),
                },
                nullable: false,
                generated: false,
            }],
        }
    }

    #[test]
    fn compares_enum_order_across_databases_without_comparing_relation_oids() {
        let source = layout(100, ["low", "high"]);

        assert!(source.has_same_columns(&layout(200, ["low", "high"])));
        assert!(!source.has_same_columns(&layout(200, ["high", "low"])));
    }
}
