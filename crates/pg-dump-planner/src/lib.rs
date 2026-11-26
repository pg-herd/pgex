mod plan;
mod selection;
mod table_layout;
pub use plan::{SchemaAction, SchemaPlan};
pub use table_layout::{SourceColumnLayout, SourceTableLayout};

use std::ffi::{CStr, c_char, c_void};

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum Profile {
    /// Match pg_dump --schema-only, including its table selection semantics.
    SchemaOnly = 0,
    Clone = 1,
    /// Include recorded schema prerequisites of selected tables.
    CloneSchemaOnly = 2,
}

#[repr(C)]
struct Settings {
    standby: bool,
    full: bool,
    select_dependencies: bool,
    selected: *const u32,
    selected_count: usize,
    snapshot: *const c_char,
}

pub struct NativePlan {
    pub actions: SchemaPlan,
    pub table_layouts: Vec<SourceTableLayout>,
    pub discovery_start: u64,
    pub sort_start: u64,
    pub actions_start: u64,
}

/// # Safety
/// Call exactly once inside a disposable planner process. Upstream can exit.
/// The connection must be live, outside a transaction and exclusively borrowed
/// through this call. The planner starts a transaction and leaves it open.
pub unsafe fn child_plan(
    standby: bool,
    profile: Profile,
    selected: &[u32],
    connection: std::ptr::NonNull<pq_sys::PGconn>,
    snapshot: &CStr,
) -> NativePlan {
    unsafe extern "C" {
        fn pgex_child_plan(
            settings: *const Settings,
            connection: *mut pq_sys::PGconn,
            ctx: *mut c_void,
            receive: unsafe extern "C" fn(*mut c_void, *const plan::EntryView),
            record: unsafe extern "C" fn(*mut c_void, *const selection::ObjectLinks, i32),
            receive_table: unsafe extern "C" fn(*mut c_void, *const table_layout::TableLayoutView),
            discovery_start: *mut u64,
            sort_start: *mut u64,
            actions_start: *mut u64,
        );
    }
    let settings = Settings {
        standby,
        full: profile == Profile::Clone,
        select_dependencies: profile != Profile::SchemaOnly,
        selected: selected.as_ptr(),
        selected_count: selected.len(),
        snapshot: snapshot.as_ptr(),
    };
    let mut archive = selection::Archive::default();
    let (mut discovery_start, mut sort_start, mut actions_start) = (0, 0, 0);
    unsafe {
        pgex_child_plan(
            &settings,
            connection.as_ptr(),
            (&raw mut archive).cast(),
            plan::receive,
            selection::record,
            table_layout::receive,
            &raw mut discovery_start,
            &raw mut sort_start,
            &raw mut actions_start,
        )
    };
    let table_layouts = std::mem::take(&mut archive.table_layouts);
    let actions = archive.finish(if settings.select_dependencies {
        selected
    } else {
        &[]
    });
    NativePlan {
        actions,
        table_layouts,
        discovery_start,
        sort_start,
        actions_start,
    }
}
