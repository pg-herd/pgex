use super::Error;
use postgres_types::{FromSql, Type};
use std::{ffi::CStr, ptr::NonNull, sync::Arc};

#[derive(Debug)]
pub(super) struct ResultHandle(pub(super) NonNull<pq_sys::PGresult>);
// A completed PGresult is immutable here; its ownership is shared only for reading.
unsafe impl Send for ResultHandle {}
unsafe impl Sync for ResultHandle {}
impl Drop for ResultHandle {
    fn drop(&mut self) {
        unsafe { pq_sys::PQclear(self.0.as_ptr()) };
    }
}

#[derive(Debug)]
pub struct RowData {
    pub(super) result: Arc<ResultHandle>,
    pub(super) index: i32,
}
pub trait RowIndex {
    fn index(self, row: &RowData) -> Result<i32, Error>;
}
impl RowIndex for usize {
    fn index(self, row: &RowData) -> Result<i32, Error> {
        if self < unsafe { pq_sys::PQnfields(row.result.0.as_ptr()) } as usize {
            Ok(self as i32)
        } else {
            Err(Error::local("Column index outside result"))
        }
    }
}
impl RowIndex for &str {
    fn index(self, row: &RowData) -> Result<i32, Error> {
        for index in 0..unsafe { pq_sys::PQnfields(row.result.0.as_ptr()) } {
            if unsafe { CStr::from_ptr(pq_sys::PQfname(row.result.0.as_ptr(), index)) }.to_bytes()
                == self.as_bytes()
            {
                return Ok(index);
            }
        }
        Err(Error::local(format!("Unknown result column {self}")))
    }
}
pub trait Row {
    fn try_get<'a, T: FromSql<'a>, I: RowIndex>(&'a self, index: I) -> Result<T, Error>;
    fn get<'a, T: FromSql<'a>, I: RowIndex>(&'a self, index: I) -> T {
        self.try_get(index).unwrap()
    }
}
impl Row for RowData {
    fn try_get<'a, T: FromSql<'a>, I: RowIndex>(&'a self, index: I) -> Result<T, Error> {
        let column = index.index(self)?;
        let result = self.result.0.as_ptr();
        let oid = unsafe { pq_sys::PQftype(result, column) };
        let kind = Type::from_oid(oid)
            .ok_or_else(|| Error::local(format!("Unsupported result type {oid}")))?;
        if !T::accepts(&kind) {
            return Err(Error::local(format!(
                "Result decoder does not accept {kind}"
            )));
        }
        let bytes = if unsafe { pq_sys::PQgetisnull(result, self.index, column) } != 0 {
            None
        } else {
            Some(unsafe {
                std::slice::from_raw_parts(
                    pq_sys::PQgetvalue(result, self.index, column).cast(),
                    pq_sys::PQgetlength(result, self.index, column) as usize,
                )
            })
        };
        T::from_sql_nullable(&kind, bytes).map_err(Error::local)
    }
}
