//! Select roots by table ownership, then walk prerequisites in the complete TOC.
use super::{SchemaPlan, SourceTableLayout};
use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
};

#[derive(Default)]
pub(crate) struct Archive {
    pub plan: SchemaPlan,
    pub table_layouts: Vec<SourceTableLayout>,
    pub data_dependencies: HashMap<i32, Vec<i32>>,
    roots: HashMap<u32, Vec<i32>>,
    partitions: HashMap<u32, Vec<u32>>,
    dependencies: HashMap<i32, Vec<i32>>,
    bootstrap: Vec<i32>,
}

#[repr(C)]
pub(crate) struct ObjectLinks {
    id: i32,
    table_oid: u32,
    partition_parent: u32,
    completes: i32,
    requires: i32,
    extension: i32,
}

pub(crate) unsafe extern "C" fn record(
    context: *mut c_void,
    object: *const ObjectLinks,
    emitted: i32,
) {
    let archive = unsafe { &mut *context.cast::<Archive>() };
    let obj = unsafe { &*object };
    if obj.id == 0 {
        archive.bootstrap.push(emitted);
    } else if emitted != 0 {
        if obj.id != emitted {
            archive.link(obj.id, emitted);
            archive.link(emitted, obj.id);
        }
    } else {
        if obj.table_oid != 0 {
            archive.roots.entry(obj.table_oid).or_default().push(obj.id);
        }
        if obj.partition_parent != 0 {
            archive
                .partitions
                .entry(obj.partition_parent)
                .or_default()
                .push(obj.table_oid);
        }
        archive.link(obj.completes, obj.id);
        archive.link(obj.id, obj.requires);
        archive.link(obj.id, obj.extension);
    }
}

// Mark on discovery so cycles, shared prerequisites and duplicate edges are
// visited once. Never rebuild archive order from the traversal.
fn reachable<T: Copy + Eq + std::hash::Hash>(
    roots: impl IntoIterator<Item = T>,
    edges: &HashMap<T, Vec<T>>,
) -> HashSet<T> {
    let mut seen = HashSet::new();
    let mut pending: Vec<_> = roots.into_iter().filter(|id| seen.insert(*id)).collect();
    while let Some(id) = pending.pop() {
        if let Some(next) = edges.get(&id) {
            pending.extend(next.iter().copied().filter(|id| seen.insert(*id)));
        }
    }
    seen
}

impl Archive {
    fn link(&mut self, from: i32, to: i32) {
        if from != 0 && to != 0 {
            self.dependencies.entry(from).or_default().push(to);
        }
    }

    pub fn finish(mut self, selected: &[u32]) -> SchemaPlan {
        if selected.is_empty() {
            return self.plan;
        }
        let tables = reachable(selected.iter().copied(), &self.partitions);
        for entry in self.plan.pre_data.iter().chain(&self.plan.post_data) {
            self.dependencies
                .entry(entry.id)
                .or_default()
                .extend(&entry.prerequisites);
        }
        for (id, deps) in self.data_dependencies {
            self.dependencies.entry(id).or_default().extend(deps);
        }
        let roots = tables
            .iter()
            .filter_map(|oid| self.roots.get(oid))
            .flatten()
            .copied();
        let keep = reachable(self.bootstrap.into_iter().chain(roots), &self.dependencies);
        self.plan.pre_data.retain(|e| keep.contains(&e.id));
        self.plan.post_data.retain(|e| keep.contains(&e.id));
        self.plan.prerequisite_table_ids = self
            .plan
            .pre_data
            .iter()
            .chain(&self.plan.post_data)
            .filter(|e| e.description == b"TABLE" && !tables.contains(&e.catalog_oid))
            .map(|e| e.id)
            .collect();
        self.plan
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn mutual_self_and_duplicate_edges_terminate() {
        let edges = [(1, vec![1, 2, 2]), (2, vec![1, 3]), (4, vec![1])].into();
        assert_eq!(super::reachable([1, 1], &edges), [1, 2, 3].into());
    }
    #[test]
    fn data_node_connects_schema_actions_but_is_never_emitted() {
        use super::{Archive, SchemaPlan};
        use crate::SchemaAction;
        let entry = |id, prerequisites| SchemaAction {
            id,
            prerequisites,
            ..Default::default()
        };
        let archive = Archive {
            plan: SchemaPlan {
                pre_data: vec![entry(8, vec![]), entry(2, vec![])],
                post_data: vec![entry(5, vec![4, 4])],
                ..Default::default()
            },
            roots: [(100, vec![5])].into(),
            data_dependencies: [(4, vec![5, 2])].into(),
            ..Default::default()
        };
        let plan = archive.finish(&[100]);
        assert_eq!(plan.pre_data, [entry(2, vec![])]);
        assert_eq!(plan.post_data, [entry(5, vec![4, 4])]);
    }
}
