use std::future::Future;

use anyhow::Result;

use crate::{
    catalog::objects::constraints::ConstraintDefinition,
    plan::{
        planner::SourcePlan,
        schema::{IndexPlan, PreCopySchema},
    },
    snapshot::SnapshotReaders,
};

pub trait Target {
    /// Copies the selected rows to the target using the supplied source readers.
    /// All target workers must finish or be stopped before returning.
    fn sync_rows(
        &self,
        readers: &SnapshotReaders<'_>,
        source_plan: &SourcePlan<'_>,
    ) -> impl Future<Output = Result<()>> + Send;
}

/// A target that can provision and populate a new database.
pub trait CloneTarget: Target {
    fn create_database(&self) -> impl Future<Output = Result<()>> + Send;

    fn create_schema_tables(
        &self,
        schema: &PreCopySchema,
    ) -> impl Future<Output = Result<()>> + Send;

    fn create_indexes(&self, indexes: &IndexPlan) -> impl Future<Output = Result<()>> + Send;

    fn create_constraints(
        &self,
        constraints: &[ConstraintDefinition],
    ) -> impl Future<Output = Result<()>> + Send;
}
