use std::io::Write;

use anyhow::{Context, Result};
use pgex::connection::Connector;
use tempfile::NamedTempFile;
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{CmdWaitFor, ExecCommand, IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};

const POSTGRES_PORT: u16 = 5432;
const TLS_ENTRYPOINT: &str = r#"
umask 077
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj /CN=localhost \
    -addext 'subjectAltName=DNS:localhost,IP:127.0.0.1,IP:::1' \
    -addext 'basicConstraints=critical,CA:FALSE' \
    -addext 'extendedKeyUsage=serverAuth' \
    -keyout /tmp/pgex-server.key -out /tmp/pgex-server.crt
chown postgres:postgres /tmp/pgex-server.key /tmp/pgex-server.crt
exec docker-entrypoint.sh postgres -c ssl=on \
    -c ssl_cert_file=/tmp/pgex-server.crt -c ssl_key_file=/tmp/pgex-server.key
"#;

pub(super) struct PostgresTestDatabase {
    container: ContainerAsync<GenericImage>,
    pub(super) connection_string: String,
    pub(super) password: String,
    pub(super) ca: NamedTempFile,
}

impl PostgresTestDatabase {
    pub(super) async fn stderr(&self) -> Result<String> {
        Ok(String::from_utf8(self.container.stderr_to_vec().await?)?)
    }

    pub(super) async fn connector(&self) -> Result<Connector> {
        Connector::new(
            &self.connection_string,
            Some(self.ca.path()),
            Some(&self.password),
        )
        .await
    }

    pub(super) async fn start() -> Result<Self> {
        let mut password_bytes = [0; 16];
        getrandom::fill(&mut password_bytes).map_err(|error| anyhow::anyhow!("{error}"))?;
        let password = format!("{:032x}", u128::from_be_bytes(password_bytes));

        let container = GenericImage::new("postgres", "17.11-bookworm")
            .with_exposed_port(POSTGRES_PORT.tcp())
            .with_entrypoint("sh")
            .with_wait_for(WaitFor::message_on_stdout(
                "PostgreSQL init process complete",
            ))
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_cmd(["-ec", TLS_ENTRYPOINT])
            .with_env_var("POSTGRES_USER", "pgex_test")
            .with_env_var("POSTGRES_DB", "pgex_snapshot_test")
            .with_env_var("POSTGRES_PASSWORD", &password)
            .start()
            .await
            .context("Start the PostgreSQL test container (requires a running Docker engine)")?;

        let host = container.get_host().await?;
        let port = container.get_host_port_ipv4(POSTGRES_PORT.tcp()).await?;
        let mut certificate = container
            .exec(
                ExecCommand::new(["cat", "/tmp/pgex-server.crt"])
                    .with_cmd_ready_condition(CmdWaitFor::exit_code(0)),
            )
            .await?;
        let mut ca = NamedTempFile::new()?;
        ca.write_all(&certificate.stdout_to_vec().await?)?;

        Ok(Self {
            container,
            connection_string: format!("postgresql://pgex_test@{host}:{port}/pgex_snapshot_test"),
            password,
            ca,
        })
    }
}

pub(super) fn sql_state(error: &pgex::connection::Error) -> Option<std::borrow::Cow<'_, str>> {
    error.as_database_error().and_then(|error| error.code())
}
