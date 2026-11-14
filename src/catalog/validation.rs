use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};

use super::TableLayout;

pub fn validate_layouts<'a>(
    source_layouts: impl IntoIterator<Item = &'a TableLayout>,
    target_layouts: &[TableLayout],
) -> Result<()> {
    let mut target_tables: BTreeMap<_, _> = target_layouts
        .iter()
        .map(|table| (&table.name, table))
        .collect();

    for source_table in source_layouts {
        let name = &source_table.name;
        let target_table = target_tables.remove(name).with_context(|| {
            format!(
                "Destination table {:?}.{:?} is missing",
                name.schema(),
                name.table()
            )
        })?;
        if !source_table.has_same_columns(target_table) {
            validate_columns(source_table, target_table).with_context(|| {
                format!("Layout mismatch for {:?}.{:?}", name.schema(), name.table())
            })?;
        }
    }

    if let Some((name, _)) = target_tables.first_key_value() {
        anyhow::bail!(
            "Destination layout has an unexpected table {:?}.{:?}",
            name.schema(),
            name.table()
        );
    }
    Ok(())
}

fn validate_columns(source: &TableLayout, target: &TableLayout) -> Result<()> {
    for (position, (source, target)) in source.columns.iter().zip(&target.columns).enumerate() {
        ensure!(
            source.name == target.name,
            "Column {} name differs (source {:?}, destination {:?})",
            position + 1,
            source.name,
            target.name
        );
        let name = &source.name;
        ensure!(
            source.data_type == target.data_type,
            "Column {name:?} type differs (source {:?}, destination {:?})",
            source.data_type,
            target.data_type
        );
        ensure!(
            source.nullable == target.nullable,
            "Column {name:?} nullability differs (source {}, destination {})",
            source.nullable,
            target.nullable
        );
        ensure!(
            source.generated == target.generated,
            "Column {name:?} generated flag differs (source {}, destination {})",
            source.generated,
            target.generated
        );
    }
    ensure!(
        source.columns.len() == target.columns.len(),
        "Column count differs (source {}, destination {})",
        source.columns.len(),
        target.columns.len()
    );
    Ok(())
}
