use std::{fmt, str::FromStr};

use anyhow::{Result, ensure};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct QualifiedTable {
    schema: String,
    table: String,
}

impl FromStr for QualifiedTable {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        let (schema, table) = value
            .split_once('.')
            .ok_or_else(|| anyhow::anyhow!("Expected schema.table"))?;
        ensure!(
            valid_identifier(schema) && valid_identifier(table),
            "Expected schema.table with simple PostgreSQL identifiers"
        );
        Ok(Self {
            schema: schema.into(),
            table: table.into(),
        })
    }
}

impl QualifiedTable {
    pub(crate) fn from_catalog(schema: String, table: String) -> Self {
        Self { schema, table }
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn table(&self) -> &str {
        &self.table
    }
}

impl fmt::Display for QualifiedTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Diagnostic formatting only. SQL identifiers are quoted by libpq on
        // the connection that executes the statement.
        write!(
            f,
            "\"{}\".\"{}\"",
            self.schema.replace('"', "\"\""),
            self.table.replace('"', "\"\"")
        )
    }
}

fn valid_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= 63
        && bytes
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$'))
}

#[cfg(test)]
mod tests {
    use super::QualifiedTable;

    #[test]
    fn displays_both_parts_and_preserves_case() {
        let table: QualifiedTable = "_Audit.Event$1".parse().unwrap();
        assert_eq!(table.to_string(), r#""_Audit"."Event$1""#);
    }

    #[test]
    fn displays_quotes_in_catalog_names() {
        let table = QualifiedTable::from_catalog("event logs".into(), r#"a"b"#.into());
        assert_eq!(table.to_string(), r#""event logs"."a""b""#);
    }

    #[test]
    fn rejects_names_outside_the_supported_identifier_syntax() {
        for name in [
            "events",
            ".events",
            "public.",
            "public.2events",
            "public.a.b",
            "public.a b",
            "public.\"events\"",
        ] {
            assert!(name.parse::<QualifiedTable>().is_err(), "accepted {name}");
        }
    }

    #[test]
    fn accepts_identifiers_up_to_63_bytes() {
        let table = "a".repeat(63);
        assert!(format!("public.{table}").parse::<QualifiedTable>().is_ok());
        assert!(
            format!("public.{table}a")
                .parse::<QualifiedTable>()
                .is_err()
        );
    }
}
