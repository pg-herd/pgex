use anyhow::{Context, Result, ensure};
use std::{
    ffi::{CStr, CString},
    path::PathBuf,
    ptr,
};

pub(in crate::connection) fn parse(input: &str) -> Result<Vec<(String, String)>> {
    let input =
        CString::new(input).map_err(|_| anyhow::anyhow!("Invalid PostgreSQL connection string"))?;
    let mut message = ptr::null_mut();
    // PQconninfoParse does not connect or insert environment/default values.
    let raw = unsafe { pq_sys::PQconninfoParse(input.as_ptr(), &mut message) };
    if !message.is_null() {
        unsafe { pq_sys::PQfreemem(message.cast()) };
    }
    ensure!(!raw.is_null(), "Invalid PostgreSQL connection string");
    // Copy before freeing libpq's array, including when a percent-decoded
    // credential is not valid UTF-8. Never silently replace password bytes.
    let copied = (|| -> Result<Vec<(String, String)>> {
        let mut values = Vec::new();
        let mut item = raw;
        unsafe {
            while !(*item).keyword.is_null() {
                if !(*item).val.is_null() {
                    let key = CStr::from_ptr((*item).keyword).to_str()?.to_owned();
                    let value = CStr::from_ptr((*item).val).to_str()?.to_owned();
                    values.push((key, value));
                }
                item = item.add(1);
            }
        }
        Ok(values)
    })();
    unsafe { pq_sys::PQconninfoFree(raw) };
    copied.map_err(|_| anyhow::anyhow!("Invalid PostgreSQL connection string"))
}

pub(in crate::connection) fn default_root_cert() -> Result<String> {
    if unsafe { pq_sys::PQlibVersion() } >= 160000 {
        return Ok("system".into());
    }
    // libpq 15 (Debian 12) predates sslrootcert=system. Use the same platform CA
    // bundle supplied by ca-certificates rather than weakening verification.
    ["/etc/ssl/certs/ca-certificates.crt", "/etc/ssl/cert.pem"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
        .context("System CA bundle not found; supply a TLS CA file")
}
