use clap::ValueEnum;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum SnapshotMode {
    #[default]
    Shared,
    PerStatement,
}
