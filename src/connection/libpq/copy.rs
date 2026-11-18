use super::{Error, PgConnection};
use bytes::Bytes;
use std::{
    ffi::{CStr, CString},
    ptr::{self, NonNull},
};

impl PgConnection {
    async fn start_copy(&mut self, sql: &str, expected: i32) -> Result<(), Error> {
        self.send(sql, None).await?;
        let result = self
            .result()
            .await?
            .ok_or_else(|| Error::local("Missing COPY startup response"))?;
        if (unsafe { pq_sys::PQresultStatus(result.0.as_ptr()) } as i32) == expected {
            return Ok(());
        }
        let error = unsafe { Error::from_result(result.0.as_ptr()) };
        while self.result().await?.is_some() {}
        Err(error)
    }
    pub async fn copy_in_raw(&mut self, sql: &str) -> Result<CopyIn<'_>, Error> {
        self.start_copy(sql, 4).await?;
        Ok(CopyIn { connection: self })
    }
    pub async fn copy_out_raw(&mut self, sql: &str) -> Result<CopyOut<'_>, Error> {
        self.start_copy(sql, 3).await?;
        Ok(CopyOut {
            connection: self,
            finished: false,
        })
    }
    async fn complete(&mut self) -> Result<u64, Error> {
        let mut count = 0;
        let mut failure = None;
        while let Some(result) = self.result().await? {
            if (unsafe { pq_sys::PQresultStatus(result.0.as_ptr()) } as i32) != 1 {
                failure.get_or_insert_with(|| unsafe { Error::from_result(result.0.as_ptr()) });
            } else {
                count = unsafe { CStr::from_ptr(pq_sys::PQcmdTuples(result.0.as_ptr())) }
                    .to_string_lossy()
                    .parse()
                    .unwrap_or(0);
            }
        }
        self.pending = false;
        failure.map_or(Ok(count), Err)
    }
}

pub struct CopyOut<'a> {
    connection: &'a mut PgConnection,
    finished: bool,
}

impl CopyOut<'_> {
    pub async fn next_chunk(&mut self) -> Result<Option<Bytes>, Error> {
        if self.finished {
            return Ok(None);
        }
        let mut retried = false;
        loop {
            let (count, bytes) = {
                let mut buffer = ptr::null_mut();
                let count =
                    unsafe { pq_sys::PQgetCopyData(self.connection.raw.as_ptr(), &mut buffer, 1) };
                let bytes = if count > 0 {
                    Some(Bytes::from_owner(CopyBuffer {
                        pointer: NonNull::new(buffer.cast()).unwrap(),
                        length: count as usize,
                    }))
                } else {
                    None
                };
                (count, bytes)
            };
            match count {
                n if n > 0 => return Ok(bytes),
                0 => {
                    if !retried {
                        if unsafe { pq_sys::PQconsumeInput(self.connection.raw.as_ptr()) } == 0 {
                            self.finished = true;
                            return Err(self.connection.error());
                        }
                        retried = true;
                    } else {
                        if let Err(error) = self.connection.input().await {
                            self.finished = true;
                            return Err(error);
                        }
                        retried = false;
                    }
                }
                -1 => {
                    let result = self.connection.complete().await;
                    self.finished = true;
                    result?;
                    return Ok(None);
                }
                _ => {
                    self.finished = true;
                    return Err(self.connection.error());
                }
            }
        }
    }
}

pub struct CopyIn<'a> {
    connection: &'a mut PgConnection,
}
impl CopyIn<'_> {
    pub async fn send(&mut self, payload: Bytes) -> Result<(), Error> {
        loop {
            match unsafe {
                pq_sys::PQputCopyData(
                    self.connection.raw.as_ptr(),
                    payload.as_ptr().cast(),
                    payload.len().try_into().map_err(Error::local)?,
                )
            } {
                1 => return self.connection.flush().await,
                0 => self.connection.flush().await?,
                _ => return Err(self.connection.error()),
            }
        }
    }
    async fn end(&mut self, error: Option<&str>) -> Result<(), Error> {
        let error = error.map(CString::new).transpose().map_err(Error::local)?;
        loop {
            match unsafe {
                pq_sys::PQputCopyEnd(
                    self.connection.raw.as_ptr(),
                    error.as_ref().map_or(ptr::null(), |error| error.as_ptr()),
                )
            } {
                1 => return self.connection.flush().await,
                0 => self.connection.flush().await?,
                _ => return Err(self.connection.error()),
            }
        }
    }
    pub async fn finish(mut self) -> Result<u64, Error> {
        self.end(None).await?;
        self.connection.complete().await
    }
    pub async fn abort(mut self, message: &str) -> Result<(), Error> {
        self.end(Some(message)).await?;
        match self.connection.complete().await {
            Err(error) if error.code().as_deref() == Some("57014") => Ok(()),
            result => result.map(|_| ()),
        }
    }
}

struct CopyBuffer {
    pointer: NonNull<u8>,
    length: usize,
}
// PQgetCopyData transfers this immutable allocation to the caller.
unsafe impl Send for CopyBuffer {}
unsafe impl Sync for CopyBuffer {}
impl AsRef<[u8]> for CopyBuffer {
    fn as_ref(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.pointer.as_ptr(), self.length) }
    }
}
impl Drop for CopyBuffer {
    fn drop(&mut self) {
        unsafe { pq_sys::PQfreemem(self.pointer.as_ptr().cast()) };
    }
}
