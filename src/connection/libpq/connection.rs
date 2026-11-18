use super::{
    Error, QueryParameter,
    row::{ResultHandle, RowData},
};
use std::{
    ffi::{CStr, CString, c_void},
    os::fd::{AsRawFd, BorrowedFd, OwnedFd, RawFd},
    ptr::NonNull,
    sync::Arc,
};
use tokio::io::unix::AsyncFd;

fn register_socket(fd: RawFd) -> Result<AsyncFd<OwnedFd>, Error> {
    let owned = unsafe { BorrowedFd::borrow_raw(fd) }.try_clone_to_owned()?;
    // This independent descriptor remains valid until AsyncFd drops it, even
    // if libpq replaces its own descriptor while retrying a connection.
    unsafe { AsyncFd::register(owned) }.map_err(Error::local)
}
pub struct PgConnection {
    socket: AsyncFd<OwnedFd>,
    pub(super) raw: NonNull<pq_sys::PGconn>,
    fatal: Box<Option<Error>>,
    pub(super) pending: bool,
}
// The client mutex provides exclusive connection access across threads.
unsafe impl Send for PgConnection {}
impl Drop for PgConnection {
    fn drop(&mut self) {
        unsafe { pq_sys::PQfinish(self.raw.as_ptr()) };
    }
}

unsafe extern "C" fn receive(context: *mut c_void, result: *const pq_sys::PGresult) {
    let error = unsafe { Error::from_result(result) };
    if matches!(error.severity.as_str(), "FATAL" | "PANIC") {
        unsafe {
            *context.cast::<Option<Error>>() = Some(error);
        }
    }
}

impl PgConnection {
    pub(in crate::connection) async fn connect(conninfo: &CStr) -> Result<Self, Error> {
        let mut connection = {
            let raw = NonNull::new(unsafe { pq_sys::PQconnectStart(conninfo.as_ptr()) })
                .ok_or_else(|| Error::local("Allocate libpq connection"))?;
            let fd = unsafe { pq_sys::PQsocket(raw.as_ptr()) };
            if fd < 0 {
                let message = unsafe { CStr::from_ptr(pq_sys::PQerrorMessage(raw.as_ptr())) }
                    .to_string_lossy()
                    .into_owned();
                unsafe { pq_sys::PQfinish(raw.as_ptr()) };
                return Err(Error::local(message));
            }
            let socket = match register_socket(fd) {
                Ok(socket) => socket,
                Err(error) => {
                    unsafe { pq_sys::PQfinish(raw.as_ptr()) };
                    return Err(error);
                }
            };
            Self {
                socket,
                raw,
                fatal: Box::new(None),
                pending: false,
            }
        };
        let mut polled = 2;
        loop {
            let current = unsafe { pq_sys::PQsocket(connection.raw.as_ptr()) };
            if current < 0 {
                return Err(connection.error());
            }
            connection.socket = register_socket(current)?;
            match polled {
                0 => return Err(connection.error()),
                1 => {
                    connection.socket.readable().await?.clear_ready();
                }
                2 => {
                    connection.socket.writable().await?.clear_ready();
                }
                3 => break,
                _ => {}
            }
            polled = unsafe { pq_sys::PQconnectPoll(connection.raw.as_ptr()) } as i32;
        }
        if unsafe { pq_sys::PQsetnonblocking(connection.raw.as_ptr(), 1) } != 0 {
            return Err(connection.error());
        }
        unsafe {
            pq_sys::PQsetNoticeReceiver(
                connection.raw.as_ptr(),
                Some(receive),
                (&mut *connection.fatal as *mut Option<Error>).cast(),
            )
        };
        if unsafe { pq_sys::PQsslInUse(connection.raw.as_ptr()) } != 1 {
            return Err(Error::local(
                "PostgreSQL connection did not establish verified TLS",
            ));
        }
        Ok(connection)
    }

    pub(super) fn error(&self) -> Error {
        self.fatal.as_ref().clone().unwrap_or_else(|| {
            Error::local(
                unsafe { CStr::from_ptr(pq_sys::PQerrorMessage(self.raw.as_ptr())) }
                    .to_string_lossy(),
            )
        })
    }
    pub(super) async fn input(&mut self) -> Result<(), Error> {
        let mut readiness = self.socket.readable().await?;
        let read = unsafe { pq_sys::PQconsumeInput(self.raw.as_ptr()) };
        // libpq can stop reading while bytes remain in the kernel. Tokio's
        // edge-triggered readiness must stay set until the descriptor is no
        // longer readable; clearing it after every read loses the only event.
        let mut descriptor = libc::pollfd {
            fd: self.socket.get_ref().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut descriptor, 1, 0) };
        if ready == 0 {
            readiness.clear_ready();
        } else if ready < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        drop(readiness);
        // PQisBusy parses idle ErrorResponse frames and invokes the receiver.
        let busy = unsafe { pq_sys::PQisBusy(self.raw.as_ptr()) };
        if let Some(error) = self.fatal.as_ref() {
            return Err(error.clone());
        }
        if read == 0 && busy != 0 {
            return Err(self.error());
        }
        Ok(())
    }
    pub(super) async fn flush(&mut self) -> Result<(), Error> {
        loop {
            match unsafe { pq_sys::PQflush(self.raw.as_ptr()) } {
                0 => return Ok(()),
                1 => {
                    tokio::select! {
                        ready = self.socket.writable() => { ready?.clear_ready(); }
                        ready = self.socket.readable() => {
                            ready?.clear_ready();
                            if unsafe { pq_sys::PQconsumeInput(self.raw.as_ptr()) } == 0 { return Err(self.error()); }
                        }
                    }
                }
                _ => return Err(self.error()),
            }
        }
    }
    pub(super) async fn result(&mut self) -> Result<Option<Arc<ResultHandle>>, Error> {
        loop {
            let read = unsafe { pq_sys::PQconsumeInput(self.raw.as_ptr()) };
            let busy = unsafe { pq_sys::PQisBusy(self.raw.as_ptr()) };
            if busy == 0 {
                break;
            }
            if read == 0 {
                return Err(self.error());
            }
            self.input().await?;
        }
        if let Some(error) = self.fatal.as_ref() {
            return Err(error.clone());
        }
        Ok(
            NonNull::new(unsafe { pq_sys::PQgetResult(self.raw.as_ptr()) })
                .map(|raw| Arc::new(ResultHandle(raw))),
        )
    }
    pub(super) async fn send(
        &mut self,
        sql: &str,
        parameters: Option<&[&(dyn QueryParameter + Sync)]>,
    ) -> Result<(), Error> {
        if self.pending {
            // A caller may cancel a query after it was sent. Drain its remaining
            // results before reusing the same session.
            while self.result().await?.is_some() {}
            self.pending = false;
        }
        let sql = CString::new(sql).map_err(Error::local)?;
        let status = if let Some(parameters) = parameters {
            self.send_parameterized_query(&sql, parameters)?
        } else {
            unsafe { pq_sys::PQsendQuery(self.raw.as_ptr(), sql.as_ptr()) }
        };
        if status == 0 {
            return Err(self.error());
        }
        self.pending = true;
        self.flush().await
    }
    pub(in crate::connection) async fn query(
        &mut self,
        sql: &str,
        parameters: Option<&[&(dyn QueryParameter + Sync)]>,
    ) -> Result<(Vec<RowData>, u64), Error> {
        self.send(sql, parameters).await?;
        let mut rows = Vec::new();
        let mut affected = 0;
        let mut failure = None;
        while let Some(result) = self.result().await? {
            match (unsafe { pq_sys::PQresultStatus(result.0.as_ptr()) } as i32) {
                1 => {
                    affected += unsafe { CStr::from_ptr(pq_sys::PQcmdTuples(result.0.as_ptr())) }
                        .to_string_lossy()
                        .parse::<u64>()
                        .unwrap_or(0);
                }
                2 => {
                    for index in 0..unsafe { pq_sys::PQntuples(result.0.as_ptr()) } {
                        rows.push(RowData {
                            result: Arc::clone(&result),
                            index,
                        });
                    }
                }
                0 => {}
                _ => {
                    failure.get_or_insert_with(|| unsafe { Error::from_result(result.0.as_ptr()) });
                }
            }
        }
        self.pending = false;
        if let Some(error) = failure {
            return Err(error);
        }
        Ok((rows, affected))
    }
    pub(in crate::connection) async fn wait_for_disconnect(&mut self) -> Error {
        loop {
            if let Err(error) = self.input().await {
                return error;
            }
        }
    }
}
