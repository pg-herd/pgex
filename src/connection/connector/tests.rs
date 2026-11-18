use std::path::Path;

use super::Connector;
use crate::connection::{libpq::parse_conninfo, options};

fn setting(connector: &Connector, key: &str) -> String {
    let settings = parse_conninfo(connector.options.conninfo.to_str().unwrap()).unwrap();
    options::get(&settings, key).unwrap_or_default().to_owned()
}

#[tokio::test]
async fn configured_user_takes_precedence_over_the_url_user() {
    let connector = Connector::with_credentials(
        "postgresql://url_user@localhost/db",
        None,
        Some("configured_user"),
        None,
    )
    .await
    .unwrap();
    assert_eq!(setting(&connector, "user"), "configured_user");
}

#[tokio::test]
async fn missing_configured_user_preserves_the_url_user() {
    let connector = Connector::new("postgresql://url_user@localhost/db", None, None)
        .await
        .unwrap();
    assert_eq!(setting(&connector, "user"), "url_user");
}

#[tokio::test]
async fn password_override_takes_precedence_over_the_url() {
    let connector = Connector::new("postgresql://user:url@localhost/db", None, Some("override"))
        .await
        .unwrap();
    assert_eq!(setting(&connector, "password"), "override");
}

#[tokio::test]
async fn missing_override_preserves_url_password_or_passwordless_configuration() {
    for (url, expected) in [
        ("postgresql://user:url@localhost/db", "url"),
        ("postgresql://user@localhost/db", ""),
    ] {
        let connector = Connector::new(url, None, None).await.unwrap();
        assert_eq!(setting(&connector, "password"), expected);
        assert_eq!(setting(&connector, "passfile"), "/dev/null");
    }
}

#[tokio::test]
async fn connection_requires_tls_even_if_the_url_disables_it() {
    let connector = Connector::new("postgresql://user@localhost/db?sslmode=disable", None, None)
        .await
        .unwrap();
    assert_eq!(setting(&connector, "sslmode"), "verify-full");
    assert_eq!(setting(&connector, "gssencmode"), "disable");
}

#[tokio::test]
async fn invalid_connection_string_returns_a_generic_error() {
    let error = Connector::new("invalid-connection-string", None, None)
        .await
        .err()
        .unwrap();
    assert_eq!(error.to_string(), "Invalid PostgreSQL connection string");
}

#[tokio::test]
async fn invalid_ca_file_returns_a_tls_configuration_error() {
    let ca = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let error = Connector::new("dbname=app", Some(&ca), None)
        .await
        .err()
        .unwrap();
    assert_eq!(error.to_string(), "Load TLS CA file");
}

#[tokio::test]
async fn credentials_round_trip_quotes_and_backslashes_without_changing_options() {
    let password = "password\\' sslmode=disable ";
    let connector = Connector::with_credentials(
        "postgresql://url_user:url_password@localhost/db",
        None,
        Some("configured'user\\"),
        Some(password),
    )
    .await
    .unwrap();
    assert_eq!(setting(&connector, "user"), "configured'user\\");
    assert_eq!(setting(&connector, "password"), password);
    assert_eq!(setting(&connector, "sslmode"), "verify-full");
}

#[tokio::test]
async fn preserves_raw_postgresql_startup_options() {
    let connector = Connector::new(
        "postgresql://user@localhost/db?options=-c%20role%3Dreader%20-c%20DateStyle%3DSQL,DMY",
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        setting(&connector, "options"),
        "-c role=reader -c DateStyle=SQL,DMY"
    );
}

#[tokio::test]
async fn preserves_multiple_hosts_and_their_ports() {
    let connector = Connector::new(
        "host=first.example,second.example port=5433,5434 user=reader dbname=app",
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(setting(&connector, "host"), "first.example,second.example");
    assert_eq!(setting(&connector, "port"), "5433,5434");
}

#[tokio::test]
async fn preserves_hostaddr_and_hostname_for_verified_tls() {
    let connector = Connector::new(
        "host=db.example hostaddr=127.0.0.1 user=reader dbname=app",
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(setting(&connector, "hostaddr"), "127.0.0.1");
    assert_eq!(setting(&connector, "host"), "db.example");
    assert_eq!(setting(&connector, "sslmode"), "verify-full");
}

#[tokio::test]
async fn rejects_non_utf8_percent_encoded_credentials_without_mangling_them() {
    let error = Connector::new("postgresql://user:%FF@localhost/db", None, None)
        .await
        .err()
        .unwrap();
    assert_eq!(error.to_string(), "Invalid PostgreSQL connection string");
}
