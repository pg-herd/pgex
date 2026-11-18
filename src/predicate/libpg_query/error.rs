use std::ffi::CStr;

use super::ffi::NativeError;

#[derive(Debug)]
pub(in crate::predicate) struct ParseError {
    pub message: String,
    /// UTF-8 byte offset in the input; input.len() denotes EOF.
    pub byte_offset: Option<usize>,
}

/// The native error and its message must remain alive throughout this call.
pub(super) unsafe fn copy_error(input: &str, native: &NativeError) -> ParseError {
    // PostgreSQL positions are one-based and count characters, not UTF-8 bytes.
    let byte_offset = usize::try_from(native.cursorpos)
        .ok()
        .filter(|position| *position > 0)
        .and_then(|position| {
            input
                .char_indices()
                .map(|(offset, _)| offset)
                .chain([input.len()])
                .nth(position - 1)
        });
    let message = if native.message.is_null() {
        "PostgreSQL parser failed without a diagnostic".into()
    } else {
        unsafe { CStr::from_ptr(native.message) }
            .to_string_lossy()
            .into_owned()
    };
    ParseError {
        message,
        byte_offset,
    }
}
