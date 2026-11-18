use std::ffi::CStr;

use super::{Error, PgConnection};

impl PgConnection {
    pub fn quote_literal(&self, value: &str) -> Result<String, Error> {
        self.quote(value, false)
    }

    pub fn quote_identifier(&self, value: &str) -> Result<String, Error> {
        self.quote(value, true)
    }

    pub fn quote_qualified_identifier(&self, schema: &str, name: &str) -> Result<String, Error> {
        Ok(format!(
            "{}.{}",
            self.quote_identifier(schema)?,
            self.quote_identifier(name)?
        ))
    }

    fn quote(&self, value: &str, identifier: bool) -> Result<String, Error> {
        if value.contains('\0') {
            return Err(Error::local(
                "PostgreSQL quoted value contains a NUL character",
            ));
        }
        let raw = unsafe {
            if identifier {
                pq_sys::PQescapeIdentifier(self.raw.as_ptr(), value.as_ptr().cast(), value.len())
            } else {
                pq_sys::PQescapeLiteral(self.raw.as_ptr(), value.as_ptr().cast(), value.len())
            }
        };
        if raw.is_null() {
            return Err(self.error());
        }
        let quoted = unsafe { CStr::from_ptr(raw) }
            .to_str()
            .map(str::to_owned)
            .map_err(Error::local);
        unsafe { pq_sys::PQfreemem(raw.cast()) };
        quoted
    }
}
