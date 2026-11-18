use std::{borrow::Cow, ffi::CStr, fmt};

#[derive(Clone, Debug)]
pub struct Error {
    pub severity: String,
    pub sqlstate: Option<String>,
    pub message: String,
    pub detail: String,
}

impl Error {
    pub fn local(message: impl ToString) -> Self {
        Self {
            severity: String::new(),
            sqlstate: None,
            message: message.to_string(),
            detail: String::new(),
        }
    }
    pub fn as_database_error(&self) -> Option<&Self> {
        self.sqlstate.as_ref().map(|_| self)
    }
    pub fn code(&self) -> Option<Cow<'_, str>> {
        self.sqlstate.as_deref().map(Cow::Borrowed)
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    pub(super) unsafe fn from_result(result: *const pq_sys::PGresult) -> Self {
        fn field(result: *const pq_sys::PGresult, code: u8) -> String {
            let value = unsafe { pq_sys::PQresultErrorField(result, code.into()) };
            if value.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(value) }
                    .to_string_lossy()
                    .into_owned()
            }
        }
        let code = field(result, b'C');
        Self {
            severity: field(result, b'V'),
            sqlstate: (!code.is_empty()).then_some(code),
            message: field(result, b'M'),
            detail: field(result, b'D'),
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(code) = &self.sqlstate {
            write!(f, "{} [{}]: {}", self.severity, code, self.message)?;
        } else {
            f.write_str(&self.message)?;
        }
        if !self.detail.is_empty() {
            write!(f, ": {}", self.detail)?;
        }
        Ok(())
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::local(error)
    }
}
