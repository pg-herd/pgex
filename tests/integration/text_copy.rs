use anyhow::Result;
use futures_util::TryStreamExt;
use pgex::connection::Client;
use tokio::process::Command;

use super::postgres::PostgresTestDatabase;

#[tokio::test]
async fn preserves_values_with_text_copy_across_different_session_settings() -> Result<()> {
    let (source, target) =
        tokio::try_join!(PostgresTestDatabase::start(), PostgresTestDatabase::start())?;
    let source_session = source.connector().await?.connect().await?;
    let target_session = target.connector().await?.connect().await?;
    let schema = "CREATE TABLE public.copy_values (
        id integer PRIMARY KEY, label text, day date, instant timestamptz,
        local_time timestamp, duration interval, amount money,
        float_single real, float_double double precision, raw bytea,
        labels text[], document jsonb
    )";
    source_session.client().batch_execute(schema).await?;
    target_session.client().batch_execute(schema).await?;
    source_session
        .client()
        .batch_execute(
            r#"
            SET lc_monetary = 'C';
            INSERT INTO public.copy_values VALUES (
                1, E'café 雪\tline\nreturn\rslash\\literal \\N', DATE '2032-07-04',
                TIMESTAMPTZ '2032-07-04 12:34:56.123456+03',
                TIMESTAMP '2032-07-04 12:34:56.123456',
                INTERVAL '-1 mons -2 days -03:04:05.678901',
                1234567.89::numeric::money, 1.2345678::real,
                1.2345678901234567::double precision, decode('00ff095c0a', 'hex'),
                ARRAY['', NULL, E'\\N', E'tab\tnewline\n', '雪'],
                '{"text":"雪","values":[null,true,1.234567890123456789]}'
            );
            INSERT INTO public.copy_values VALUES (
                2, repeat(E'café\t\n\\', 40000), DATE '0001-01-02 BC',
                TIMESTAMPTZ 'infinity', TIMESTAMP '-infinity',
                INTERVAL '1 mons -2 days 03:04:05.678901',
                (-1234567.89)::numeric::money, '-0'::real,
                '-0'::double precision, ''::bytea, ARRAY[]::text[], '{}'
            );
            INSERT INTO public.copy_values (id, label) VALUES (3, ''), (4, NULL);
            "#,
        )
        .await?;

    // Image locales vary; exercise a different monetary locale when available.
    let target_money_locale = available_money_locale(target_session.client()).await;
    let source_url = format!(
        "{}?options=-c%20DateStyle%3DSQL,DMY\
         %20-c%20IntervalStyle%3Dsql_standard%20-c%20extra_float_digits%3D-3\
         %20-c%20TimeZone%3DAsia/Jerusalem%20-c%20log_statement%3Dall",
        source.connection_string
    );
    let target_url = format!(
        "{}?options=-c%20DateStyle%3DPostgres,MDY\
         %20-c%20IntervalStyle%3Diso_8601%20-c%20TimeZone%3DAmerica/New_York\
         %20-c%20lc_monetary%3D{target_money_locale}%20-c%20log_statement%3Dall",
        target.connection_string
    );
    let output = Command::new(env!("CARGO_BIN_EXE_pgex"))
        .args(["--source", &source_url, "--target", &target_url])
        .arg("--ca")
        .arg(source.ca.path())
        .arg("--target-ca")
        .arg(target.ca.path())
        .env("PGEX_SOURCE_PASSWORD", &source.password)
        .env("PGEX_TARGET_PASSWORD", &target.password)
        .args([
            "--tables",
            "public.copy_values",
            "--source-reader-limit",
            "1",
            "--target-parallel-transfers",
            "1",
        ])
        .output()
        .await?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Binary output is only the test oracle: it catches text rounding and
    // escaping mistakes without formatting either database's stored values.
    let expected = stored_values(source_session.client()).await?;
    let actual = stored_values(target_session.client()).await?;
    assert!(
        actual == expected,
        "Stored values differ after text COPY (source: {} bytes, target: {} bytes)",
        expected.len(),
        actual.len()
    );
    // log_statement labels a simple Query as "statement" and an extended Execute as "execute".
    let source_log = source.stderr().await?;
    let target_log = target.stderr().await?;
    assert!(source_log.lines().any(|line| {
        line.contains("statement: COPY (SELECT") && line.ends_with("TO STDOUT (FORMAT TEXT)")
    }));
    assert!(target_log.lines().any(|line| {
        line.contains("statement: COPY ") && line.ends_with("FROM STDIN (FORMAT TEXT)")
    }));
    source_session.close().await;
    target_session.close().await;
    Ok(())
}

async fn stored_values(client: &Client) -> Result<Vec<u8>> {
    let mut connection = client.connection().await?;
    let stream = connection
        .get()
        .await?
        .copy_out_raw(
            "COPY (SELECT * FROM public.copy_values ORDER BY id) TO STDOUT (FORMAT BINARY)",
        )
        .await?;
    tokio::pin!(stream);
    let mut values = Vec::new();
    while let Some(chunk) = stream.try_next().await? {
        values.extend_from_slice(&chunk);
    }
    Ok(values)
}

async fn available_money_locale(client: &Client) -> &'static str {
    for locale in ["de_DE.UTF-8", "fr_FR.UTF-8", "en_US.UTF-8"] {
        if client
            .execute(
                "SELECT pg_catalog.set_config('lc_monetary', $1, false)",
                &[&locale],
            )
            .await
            .is_ok()
        {
            return locale;
        }
    }
    "C"
}
