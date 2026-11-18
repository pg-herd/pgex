use std::{ffi::CString, path::Path, time::Duration};

use anyhow::{Context, Result, ensure};

use super::libpq::{default_root_cert, parse_conninfo};

#[derive(Clone)]
pub(super) struct ConnectionOptions {
    pub(super) conninfo: CString,
    pub(super) timeout: Duration,
}

pub(super) async fn resolve(
    input: &str,
    ca: Option<&Path>,
    user: Option<&str>,
    password: Option<&str>,
) -> Result<ConnectionOptions> {
    let mut settings = parse_conninfo(input)?;
    if let Some(user) = user {
        set(&mut settings, "user", user.to_owned());
    }
    if let Some(password) = password {
        set(&mut settings, "password", password.to_owned());
    }
    // Explicit empty conninfo values bypass PGUSER/PGDATABASE. Libpq then
    // resolves an empty user to the OS username and an empty database to that
    // resolved user, preserving pgex's defaults without another OS lookup crate.
    if get(&settings, "user").is_none() {
        set(&mut settings, "user", String::new());
    }
    if get(&settings, "dbname").is_none() {
        set(&mut settings, "dbname", String::new());
    }
    if get(&settings, "host").is_none() && get(&settings, "hostaddr").is_none() {
        set(&mut settings, "host", "localhost".into());
    }
    // Explicit empty values in conninfo prevent libpq's ambient PGPASSWORD and
    // password-file defaults from changing pgex's resolved credentials.
    if get(&settings, "password").is_none() {
        set(&mut settings, "password", String::new());
    }
    if get(&settings, "passfile").is_none() {
        set(&mut settings, "passfile", "/dev/null".into());
    }
    set(&mut settings, "sslmode", "verify-full".into());
    set(&mut settings, "gssencmode", "disable".into());
    if let Some(ca) = ca {
        validate_ca(ca).await.context("Load TLS CA file")?;
        set(
            &mut settings,
            "sslrootcert",
            ca.to_str().context("TLS CA path must be UTF-8")?.to_owned(),
        );
    } else if get(&settings, "sslrootcert").is_none() {
        set(&mut settings, "sslrootcert", default_root_cert()?);
    }
    let timeout = get(&settings, "connect_timeout")
        .map(|value| value.trim().parse::<u64>())
        .transpose()
        .context("connect_timeout must be a nonnegative number of seconds")?
        .filter(|seconds| *seconds != 0)
        .map_or(Duration::from_secs(30), Duration::from_secs);
    let conninfo = settings
        .iter()
        .map(|(key, value)| {
            format!(
                "{key}='{}'",
                value.replace('\\', "\\\\").replace('\'', "\\'")
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    Ok(ConnectionOptions {
        conninfo: CString::new(conninfo)?,
        timeout,
    })
}

pub(crate) fn validate_connection_string(input: &str) -> Result<()> {
    parse_conninfo(input).map(|_| ())
}

pub(super) fn get<'a>(settings: &'a [(String, String)], key: &str) -> Option<&'a str> {
    settings
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

fn set(settings: &mut Vec<(String, String)>, key: &str, value: String) {
    match settings.iter_mut().find(|(name, _)| name == key) {
        Some((_, current)) => *current = value,
        None => settings.push((key.to_owned(), value)),
    }
}

async fn validate_ca(path: &Path) -> Result<()> {
    let bytes = tokio::fs::read(path).await?;
    let pem = std::str::from_utf8(&bytes)?;
    ensure!(
        pem.contains("-----BEGIN CERTIFICATE-----") && pem.contains("-----END CERTIFICATE-----"),
        "CA file contains no PEM certificates"
    );
    // libpq/OpenSSL validates the actual certificates during connection setup.
    Ok(())
}
