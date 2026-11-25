use std::ffi::{c_char, c_int};

// These layouts mirror pg_query.h from the pinned pg_query 18.1.0 release.
#[repr(C)]
pub(super) struct NativeError {
    pub(super) message: *mut c_char,
    pub(super) funcname: *mut c_char,
    pub(super) filename: *mut c_char,
    pub(super) lineno: c_int,
    pub(super) cursorpos: c_int,
    pub(super) context: *mut c_char,
}

#[repr(C)]
struct SplitStatement {
    location: c_int,
    len: c_int,
}

#[repr(C)]
pub(super) struct SplitResult {
    statements: *mut *mut SplitStatement,
    statement_count: c_int,
    stderr_buffer: *mut c_char,
    pub(super) error: *mut NativeError,
}

#[repr(C)]
pub(super) struct ScanToken {
    pub(super) start: c_int,
    pub(super) end: c_int,
    pub(super) token: c_int,
    pub(super) keyword_kind: c_int,
}

#[repr(C)]
pub(super) struct ScanResult {
    pub(super) tokens: *mut ScanToken,
    pub(super) n_tokens: c_int,
    pub(super) error: *mut NativeError,
}

unsafe extern "C" {
    pub(super) fn pg_query_scan_tokens(input: *const c_char) -> ScanResult;
    pub(super) fn pg_query_free_scan_tokens_result(result: ScanResult);
    pub(super) fn pg_query_token_name(token: c_int) -> *const c_char;
    pub(super) fn pg_query_split_with_parser(input: *const c_char) -> SplitResult;
    pub(super) fn pg_query_free_split_result(result: SplitResult);
}
