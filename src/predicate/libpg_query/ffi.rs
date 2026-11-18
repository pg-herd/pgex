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
pub(super) struct Protobuf {
    pub(super) len: usize,
    pub(super) data: *mut c_char,
}

#[repr(C)]
pub(super) struct ParseResult {
    pub(super) parse_tree: Protobuf,
    pub(super) stderr_buffer: *mut c_char,
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
    pub(super) fn pg_query_parse_protobuf(input: *const c_char) -> ParseResult;
    pub(super) fn pg_query_free_protobuf_parse_result(result: ParseResult);
}
