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

#[cfg(test)]
mod tests {
    use std::{
        ffi::{CStr, CString},
        process::Command,
    };

    use crate::connection::options;

    #[tokio::test]
    async fn defaults_ignore_pguser_and_pgdatabase() {
        // Set ambient defaults only in a subprocess, never by mutating the
        // environment of the multithreaded test runner.
        const CHILD: &str = "PGEX_TEST_LIBPQ_DEFAULTS_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "connection::libpq::conninfo::tests::defaults_ignore_pguser_and_pgdatabase",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env("PGUSER", "ambient_user_must_be_ignored")
                .env("PGDATABASE", "ambient_database_must_be_ignored")
                .output()
                .unwrap();
            assert!(
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let output = Command::new("id").arg("-un").output().unwrap();
        assert!(output.status.success());
        let os_user = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        for (input, user, expected_user, expected_db) in [
            ("host=localhost", None, os_user.as_str(), os_user.as_str()),
            (
                "host=localhost dbname=explicit_db",
                None,
                os_user.as_str(),
                "explicit_db",
            ),
            ("host=localhost user=url_user", None, "url_user", "url_user"),
            (
                "host=localhost user=url_user",
                Some("configured_user"),
                "configured_user",
                "configured_user",
            ),
            (
                "host=localhost user=url_user dbname=explicit_db",
                Some("configured_user"),
                "configured_user",
                "explicit_db",
            ),
        ] {
            let options = options::resolve(input, None, user, None).await.unwrap();
            // Invalid port stops startup before any network connection. Libpq
            // resolves the connection identity before validating the port.
            let conninfo = CString::new(format!(
                "{} hostaddr=127.0.0.1 port=invalid",
                options.conninfo.to_str().unwrap()
            ))
            .unwrap();
            let connection = unsafe { pq_sys::PQconnectStart(conninfo.as_ptr()) };
            assert!(!connection.is_null());
            let actual_user = unsafe { CStr::from_ptr(pq_sys::PQuser(connection)) }
                .to_str()
                .unwrap()
                .to_owned();
            let actual_db = unsafe { CStr::from_ptr(pq_sys::PQdb(connection)) }
                .to_str()
                .unwrap()
                .to_owned();
            unsafe { pq_sys::PQfinish(connection) };
            assert_eq!(actual_user, expected_user, "{input}");
            assert_eq!(actual_db, expected_db, "{input}");
        }
    }
}
