use super::{Error, PgConnection};
use bytes::BytesMut;
use postgres_types::{IsNull, ToSql, Type};
use std::{
    ffi::{CStr, c_char},
    ptr,
};

pub trait QueryParameter: ToSql {
    fn parameter_type(&self) -> Result<Type, Error>;
}
impl<T: ToSql> QueryParameter for T {
    fn parameter_type(&self) -> Result<Type, Error> {
        [
            Type::BOOL,
            Type::INT4,
            Type::INT8,
            Type::DATE,
            Type::INT4_ARRAY,
            Type::INT8_ARRAY,
            Type::TEXT_ARRAY,
            Type::TEXT,
        ]
        .into_iter()
        .find(T::accepts)
        .ok_or_else(|| Error::local("Unsupported bound PostgreSQL value"))
    }
}

impl PgConnection {
    pub(super) fn send_parameterized_query(
        &mut self,
        sql: &CStr,
        parameters: &[&(dyn QueryParameter + Sync)],
    ) -> Result<i32, Error> {
        let mut types = Vec::new();
        let mut bytes = Vec::new();
        let mut lengths = Vec::new();
        let mut formats = Vec::new();
        for value in parameters {
            let ty = value.parameter_type()?;
            let mut encoded = BytesMut::new();
            let null = matches!(
                value
                    .to_sql_checked(&ty, &mut encoded)
                    .map_err(Error::local)?,
                IsNull::Yes
            );
            types.push(ty.oid());
            lengths.push(i32::try_from(encoded.len()).map_err(Error::local)?);
            formats.push(1);
            bytes.push((null, encoded));
        }
        let values: Vec<*const c_char> = bytes
            .iter()
            .map(|(null, bytes)| {
                if *null {
                    ptr::null()
                } else {
                    bytes.as_ptr().cast()
                }
            })
            .collect();
        let status = unsafe {
            pq_sys::PQsendQueryParams(
                self.raw.as_ptr(),
                sql.as_ptr(),
                parameters.len().try_into().map_err(Error::local)?,
                types.as_ptr(),
                values.as_ptr(),
                lengths.as_ptr(),
                formats.as_ptr(),
                1,
            )
        };
        Ok(status)
    }
}
