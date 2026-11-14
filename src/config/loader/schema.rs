use std::{
    collections::BTreeMap,
    fmt,
    num::{NonZeroU32, NonZeroUsize},
    path::PathBuf,
};

use serde::{Deserialize, Deserializer, de::Visitor};

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Document {
    pub source: Source,
    pub target: Target,
    pub params: BTreeMap<String, Text>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Source {
    pub connection: Connection,
    pub session: SourceSession,
    pub snapshot: Snapshot,
    pub selection: Selection,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Connection {
    pub url: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub ca_file: Option<PathBuf>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct SourceSession {
    pub row_security_off: Option<bool>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Snapshot {
    pub reader_connection_limit: Option<NonZeroUsize>,
    pub client_connection_check_interval_ms: Option<u32>,
    pub exporter_timeouts: Timeouts,
    pub reader_timeouts: Timeouts,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Timeouts {
    pub transaction_ms: Option<u32>,
    pub idle_in_transaction_ms: Option<u32>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Selection {
    pub tables: Option<Vec<String>>,
    #[serde(rename = "where")]
    pub shared: Option<Text>,
    pub table_rules: Option<Vec<ColumnRule>>,
    pub table_filters: Option<BTreeMap<String, TableFilter>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ColumnRule {
    pub has_column: String,
    #[serde(rename = "where")]
    pub condition: Text,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TableFilter {
    #[serde(rename = "where")]
    pub condition: Text,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Target {
    pub connection: Connection,
    pub session: TargetSession,
    pub transfer: Transfer,
    pub delete_filters: Option<BTreeMap<String, Text>>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct TargetSession {
    pub row_security_off: Option<bool>,
    pub replica_role: Option<bool>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Transfer {
    pub workers: Option<NonZeroUsize>,
    pub commit_group_mib: Option<NonZeroU32>,
    pub validate_layouts: Option<bool>,
    pub skip_delete: Option<bool>,
}

/// Require a YAML string for predicates and raw parameter values.
pub(super) struct Text(pub String);

impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StringVisitor;

        impl Visitor<'_> for StringVisitor {
            type Value = Text;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a YAML string")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Text, E> {
                Ok(Text(value.to_owned()))
            }

            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Text, E> {
                Ok(Text(value))
            }
        }

        deserializer.deserialize_any(StringVisitor)
    }
}
