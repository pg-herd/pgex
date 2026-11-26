//! Rust-owned projection of the completed PostgreSQL archive table of contents.
use std::ffi::{CStr, c_char, c_int, c_void};

#[derive(Clone, PartialEq, prost::Message)]
pub struct SchemaPlan {
    #[prost(message, repeated, tag = "1")]
    pub pre_data: Vec<SchemaAction>,
    #[prost(message, repeated, tag = "2")]
    pub post_data: Vec<SchemaAction>,
    /// Dump IDs of prerequisite table definitions outside the requested COPY set.
    #[prost(int32, repeated, tag = "3")]
    pub prerequisite_table_ids: Vec<i32>,
}

#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct SchemaAction {
    #[prost(int32, tag = "1")]
    pub id: i32,
    #[prost(uint32, tag = "2")]
    pub catalog_table: u32,
    #[prost(uint32, tag = "3")]
    pub catalog_oid: u32,
    #[prost(int32, tag = "4")]
    pub archive_section: i32,
    #[prost(int32, tag = "5")]
    pub restore_pass: i32,
    #[prost(bytes = "vec", tag = "6")]
    pub tag: Vec<u8>,
    #[prost(bytes = "vec", tag = "7")]
    pub description: Vec<u8>,
    #[prost(bytes = "vec", optional, tag = "8")]
    pub definition: Option<Vec<u8>>,
    #[prost(bytes = "vec", optional, tag = "9")]
    pub drop_stmt: Option<Vec<u8>>,
    #[prost(bytes = "vec", optional, tag = "10")]
    pub copy_stmt: Option<Vec<u8>>,
    #[prost(bytes = "vec", optional, tag = "11")]
    pub namespace: Option<Vec<u8>>,
    #[prost(bytes = "vec", optional, tag = "12")]
    pub owner: Option<Vec<u8>>,
    #[prost(bytes = "vec", optional, tag = "13")]
    pub tablespace: Option<Vec<u8>>,
    #[prost(bytes = "vec", optional, tag = "14")]
    pub tableam: Option<Vec<u8>>,
    #[prost(uint32, tag = "15")]
    pub relkind: u32,
    #[prost(int32, repeated, tag = "16")]
    pub prerequisites: Vec<i32>,
}

#[repr(C)]
pub(crate) struct EntryView {
    id: c_int,
    section: c_int,
    effective_section: c_int,
    restore_pass: c_int,
    catalog_table: u32,
    catalog_oid: u32,
    tag: *const c_char,
    description: *const c_char,
    definition: *const c_char,
    drop_stmt: *const c_char,
    copy_stmt: *const c_char,
    namespace: *const c_char,
    owner: *const c_char,
    tablespace: *const c_char,
    tableam: *const c_char,
    dependencies: *const c_int,
    dependency_count: c_int,
    relkind: c_char,
}

pub(crate) unsafe extern "C" fn receive(context: *mut c_void, entry: *const EntryView) {
    // C calls synchronously with a completed TOC entry. Text uses
    // the source client encoding; preserving bytes also preserves empty vs NULL.
    let archive = unsafe { &mut *context.cast::<super::selection::Archive>() };
    let entry = unsafe { &*entry };
    let copy =
        |p: *const c_char| (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_bytes().to_vec());
    let prerequisites = if entry.dependency_count == 0 {
        vec![]
    } else {
        unsafe { std::slice::from_raw_parts(entry.dependencies, entry.dependency_count as usize) }
            .to_vec()
    };
    if entry.effective_section == 3 {
        archive.data_dependencies.insert(entry.id, prerequisites);
        return;
    }
    let actions = if entry.effective_section == 2 {
        &mut archive.plan.pre_data
    } else {
        &mut archive.plan.post_data
    };
    actions.push(SchemaAction {
        id: entry.id,
        catalog_table: entry.catalog_table,
        catalog_oid: entry.catalog_oid,
        archive_section: entry.section,
        restore_pass: entry.restore_pass,
        tag: copy(entry.tag).unwrap_or_default(),
        description: copy(entry.description).unwrap_or_default(),
        definition: copy(entry.definition),
        drop_stmt: copy(entry.drop_stmt),
        copy_stmt: copy(entry.copy_stmt),
        namespace: copy(entry.namespace),
        owner: copy(entry.owner),
        tablespace: copy(entry.tablespace),
        tableam: copy(entry.tableam),
        relkind: u32::from(entry.relkind as u8),
        prerequisites,
    });
}
