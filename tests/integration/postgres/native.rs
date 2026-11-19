use std::{
    fs::File,
    io::Write,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command as StdCommand, Stdio},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use tempfile::{NamedTempFile, TempDir};
use tokio::{process::Command, time::Instant};

use super::{Backend, PostgresTestDatabase};

pub(super) struct NativePostgres {
    process: Child,
    directory: TempDir,
    log: PathBuf,
    pg_ctl: PathBuf,
}

impl NativePostgres {
    pub(super) async fn stderr(&self) -> Result<String> {
        Ok(tokio::fs::read_to_string(&self.log).await?)
    }
}

impl Drop for NativePostgres {
    fn drop(&mut self) {
        let stopped = StdCommand::new(&self.pg_ctl)
            .arg("-D")
            .arg(self.directory.path().join("data"))
            .args(["stop", "--mode=immediate", "--wait", "--timeout=5"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !stopped {
            let _ = self.process.kill();
        }
        let _ = self.process.wait();
    }
}

pub(super) async fn start(bin: &Path, password: String) -> Result<PostgresTestDatabase> {
    let directory = tempfile::Builder::new()
        .prefix("pgex-postgres-")
        .tempdir()?;
    let data = directory.path().join("data");
    let password_file = directory.path().join("password");
    tokio::fs::write(&password_file, &password).await?;
    run(
        Command::new(bin.join("initdb"))
            .arg("-D")
            .arg(&data)
            .args([
                "--username=pgex_test",
                "--auth-host=scram-sha-256",
                "--auth-local=trust",
                "--encoding=UTF8",
                "--no-locale",
                "--pwfile",
            ])
            .arg(&password_file),
        "Initialize the native PostgreSQL test database",
    )
    .await?;
    tokio::fs::remove_file(password_file).await?;

    let certificate = directory.path().join("server.crt");
    let key = directory.path().join("server.key");
    run(
        Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-days",
                "1",
                "-subj",
                "/CN=localhost",
                "-addext",
                "subjectAltName=DNS:localhost,IP:127.0.0.1,IP:::1",
                "-addext",
                "basicConstraints=critical,CA:FALSE",
                "-addext",
                "extendedKeyUsage=serverAuth",
                "-keyout",
            ])
            .arg(&key)
            .arg("-out")
            .arg(&certificate),
        "Generate the native PostgreSQL test certificate",
    )
    .await?;
    let mut ca = NamedTempFile::new()?;
    ca.write_all(&tokio::fs::read(&certificate).await?)?;

    // Reserve the port during initialization, then release it just before startup.
    let reservation = TcpListener::bind("127.0.0.1:0")?;
    let port = reservation.local_addr()?.port().to_string();
    let log = directory.path().join("postgres.log");
    let log_file = File::create(&log)?;
    let mut command = StdCommand::new(bin.join("postgres"));
    command
        .arg("-D")
        .arg(&data)
        .args(["-h", "127.0.0.1", "-p", &port])
        .args(["-c", "unix_socket_directories=", "-c", "ssl=on"])
        .arg("-c")
        .arg(format!("ssl_cert_file={}", certificate.display()))
        .arg("-c")
        .arg(format!("ssl_key_file={}", key.display()))
        .stdin(Stdio::null())
        .stdout(log_file.try_clone()?)
        .stderr(log_file);
    drop(reservation);
    let process = command
        .spawn()
        .context("Start native PostgreSQL for tests")?;
    // Own the process immediately so every error below also stops the server.
    let mut native = NativePostgres {
        process,
        directory,
        log,
        pg_ctl: bin.join("pg_ctl"),
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = native.process.try_wait()? {
            bail!(
                "Native PostgreSQL exited during startup ({status}):\n{}",
                native.stderr().await?
            );
        }
        if Command::new(bin.join("pg_isready"))
            .args(["-h", "127.0.0.1", "-p", &port, "-U", "pgex_test", "-q"])
            .kill_on_drop(true)
            .status()
            .await?
            .success()
        {
            break;
        }
        if Instant::now() >= deadline {
            bail!(
                "Native PostgreSQL did not become ready:\n{}",
                native.stderr().await?
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    run(
        Command::new(bin.join("createdb"))
            .args([
                "-h",
                "127.0.0.1",
                "-p",
                &port,
                "-U",
                "pgex_test",
                "--maintenance-db=postgres",
                "pgex_snapshot_test",
            ])
            .env("PGPASSWORD", &password)
            .env("PGSSLMODE", "verify-full")
            .env("PGSSLROOTCERT", &certificate),
        "Create the native PostgreSQL test database over verified TLS",
    )
    .await?;

    Ok(PostgresTestDatabase {
        backend: Backend::Native(native),
        connection_string: format!("postgresql://pgex_test@127.0.0.1:{port}/pgex_snapshot_test"),
        password,
        ca,
    })
}

async fn run(command: &mut Command, operation: &str) -> Result<()> {
    let output = command
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| operation.to_owned())?;
    if !output.status.success() {
        bail!("{operation}:\n{}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}
