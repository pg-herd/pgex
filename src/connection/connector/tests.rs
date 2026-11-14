use std::path::Path;

use super::{Connector, SslMode};

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
    assert_eq!(connector.config.get_user(), Some("configured_user"));
}

#[tokio::test]
async fn missing_configured_user_preserves_the_url_user() {
    let connector =
        Connector::with_credentials("postgresql://url_user@localhost/db", None, None, None)
            .await
            .unwrap();
    assert_eq!(connector.config.get_user(), Some("url_user"));
}

#[tokio::test]
async fn password_override_takes_precedence_over_the_url() {
    let connector = Connector::new("postgresql://user:url@localhost/db", None, Some("override"))
        .await
        .unwrap();
    assert_eq!(
        connector.config.get_password(),
        Some(b"override".as_slice())
    );
}

#[tokio::test]
async fn missing_override_preserves_url_password_or_passwordless_configuration() {
    for (source, expected) in [
        (
            "postgresql://user:url@localhost/db",
            Some(b"url".as_slice()),
        ),
        ("postgresql://user@localhost/db", None),
    ] {
        let connector = Connector::new(source, None, None).await.unwrap();
        assert_eq!(connector.config.get_password(), expected);
    }
}

#[tokio::test]
async fn connection_requires_tls_even_if_the_url_disables_it() {
    let connector = Connector::new("postgresql://user@localhost/db?sslmode=disable", None, None)
        .await
        .unwrap();
    assert_eq!(connector.config.get_ssl_mode(), SslMode::Require);
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
