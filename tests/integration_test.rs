use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio_postgres::{Client, NoTls, SimpleQueryMessage};

use postgresql_trino_gateway::config::Config;
use postgresql_trino_gateway::handler::GatewayHandlerFactory;
use postgresql_trino_gateway::query_extended::GatewayExtendedQueryHandler;
use postgresql_trino_gateway::query_simple::GatewayQueryHandler;
use postgresql_trino_gateway::startup::GatewayStartupHandler;

/// Start a gateway on a random port, return the address.
/// The gateway runs as a background tokio task.
async fn start_gateway(config: Config) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let config = Arc::new(config);
    let factory = Arc::new(GatewayHandlerFactory {
        startup: Arc::new(GatewayStartupHandler {
            config: config.clone(),
        }),
        query: Arc::new(GatewayQueryHandler),
        extended_query: Arc::new(GatewayExtendedQueryHandler),
    });

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((socket, _)) => {
                    let factory = factory.clone();
                    tokio::spawn(async move {
                        let _ = pgwire::tokio::process_socket(socket, None, factory).await;
                    });
                }
                Err(_) => break,
            }
        }
    });

    addr
}

/// Connect to the gateway with tokio-postgres.
async fn connect(addr: SocketAddr) -> Client {
    let conn_str = format!(
        "host={} port={} user=trino dbname=test",
        addr.ip(),
        addr.port()
    );
    let (client, connection) = tokio_postgres::connect(&conn_str, NoTls).await.unwrap();
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });
    client
}

/// Extract data rows from simple_query results.
fn extract_rows(messages: Vec<SimpleQueryMessage>) -> Vec<tokio_postgres::SimpleQueryRow> {
    messages
        .into_iter()
        .filter_map(|m| match m {
            SimpleQueryMessage::Row(row) => Some(row),
            _ => None,
        })
        .collect()
}

/// Default config for tests that don't need Trino.
fn test_config() -> Config {
    Config {
        listen_addr: "127.0.0.1:0".to_string(),
        trino_host: "localhost".to_string(),
        trino_port: 8080,
        trino_catalog: "memory".to_string(),
        trino_schema: "default".to_string(),
        trino_user: "trino".to_string(),
        trino_ssl: false,
        trino_ssl_insecure: false,
    }
}

/// Config pointing at real Trino (from env vars), or None if not available.
fn trino_config() -> Option<Config> {
    let host = std::env::var("TRINO_HOST").ok()?;
    let port: u16 = std::env::var("TRINO_PORT").ok()?.parse().ok()?;
    let ssl = std::env::var("TRINO_SSL")
        .ok()
        .map(|v| v == "true")
        .unwrap_or(false);
    let ssl_insecure = std::env::var("TRINO_SSL_INSECURE")
        .ok()
        .map(|v| v == "true")
        .unwrap_or(false);
    let catalog = std::env::var("TRINO_CATALOG").unwrap_or_else(|_| "tpch".to_string());
    let schema = std::env::var("TRINO_SCHEMA").unwrap_or_else(|_| "sf1".to_string());
    Some(Config {
        listen_addr: "127.0.0.1:0".to_string(),
        trino_host: host,
        trino_port: port,
        trino_catalog: catalog,
        trino_schema: schema,
        trino_user: "trino".to_string(),
        trino_ssl: ssl,
        trino_ssl_insecure: ssl_insecure,
    })
}

// ---------------------------------------------------------------------------
// Intercept tests (no Trino needed)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_select_version() {
    let addr = start_gateway(test_config()).await;
    let client = connect(addr).await;
    let rows = extract_rows(client.simple_query("SELECT version()").await.unwrap());
    let version = rows[0].get(0).unwrap();
    assert!(
        version.contains("PostgreSQL 16.6"),
        "version() = '{version}'"
    );
}

#[tokio::test]
async fn test_show_server_version() {
    let addr = start_gateway(test_config()).await;
    let client = connect(addr).await;
    let rows = extract_rows(client.simple_query("SHOW server_version").await.unwrap());
    let version = rows[0].get(0).unwrap();
    assert_eq!(version, "16.6");
}

#[tokio::test]
async fn test_set_command() {
    let addr = start_gateway(test_config()).await;
    let client = connect(addr).await;
    client.batch_execute("SET extra_float_digits = 3").await.unwrap();
    client.batch_execute("SET DateStyle = 'ISO, MDY'").await.unwrap();
    client.batch_execute("SET client_encoding = 'UTF8'").await.unwrap();
}

#[tokio::test]
async fn test_transaction_commands() {
    let addr = start_gateway(test_config()).await;
    let client = connect(addr).await;
    client.batch_execute("BEGIN").await.unwrap();
    client.batch_execute("COMMIT").await.unwrap();
    client.batch_execute("BEGIN READ ONLY").await.unwrap();
    client.batch_execute("ROLLBACK").await.unwrap();
}

#[tokio::test]
async fn test_current_database() {
    let addr = start_gateway(test_config()).await;
    let client = connect(addr).await;
    let rows = extract_rows(client.simple_query("SELECT current_database()").await.unwrap());
    assert_eq!(rows.len(), 1);
}

#[tokio::test]
async fn test_pg_is_in_recovery() {
    let addr = start_gateway(test_config()).await;
    let client = connect(addr).await;
    let rows = extract_rows(
        client
            .simple_query("SELECT pg_catalog.pg_is_in_recovery()")
            .await
            .unwrap(),
    );
    let val = rows[0].get(0).unwrap();
    assert_eq!(val, "false");
}

#[tokio::test]
async fn test_show_standard_conforming_strings() {
    let addr = start_gateway(test_config()).await;
    let client = connect(addr).await;
    let rows = extract_rows(
        client
            .simple_query("SHOW standard_conforming_strings")
            .await
            .unwrap(),
    );
    let val = rows[0].get(0).unwrap();
    assert_eq!(val, "on");
}

#[tokio::test]
async fn test_pg_type_catalog() {
    let addr = start_gateway(test_config()).await;
    let client = connect(addr).await;
    // The gateway returns all pg_type columns regardless of the SELECT list:
    //   nspname(0), oid(1), typname(2), typtype(3), typnotnull(4), elemtypoid(5)
    let rows = extract_rows(
        client
            .simple_query("SELECT * FROM pg_type")
            .await
            .unwrap(),
    );
    assert!(
        rows.len() > 40,
        "Expected at least 40 types, got {}",
        rows.len()
    );

    // typname is at index 2
    let type_names: Vec<&str> = rows.iter().map(|r| r.get(2).unwrap()).collect();
    assert!(type_names.contains(&"bool"));
    assert!(type_names.contains(&"int4"));
    assert!(type_names.contains(&"varchar"));
    assert!(type_names.contains(&"_int4")); // array type
}

#[tokio::test]
async fn test_pg_namespace_catalog() {
    let addr = start_gateway(test_config()).await;
    let client = connect(addr).await;
    // The gateway returns all pg_namespace columns: oid(0), nspname(1)
    let rows = extract_rows(
        client
            .simple_query("SELECT * FROM pg_namespace")
            .await
            .unwrap(),
    );
    // nspname is at index 1
    let names: Vec<&str> = rows.iter().map(|r| r.get(1).unwrap()).collect();
    assert!(names.contains(&"pg_catalog"));
    assert!(names.contains(&"public"));
}

#[tokio::test]
async fn test_discard_and_deallocate() {
    let addr = start_gateway(test_config()).await;
    let client = connect(addr).await;
    client.batch_execute("DISCARD ALL").await.unwrap();
    client.batch_execute("DEALLOCATE ALL").await.unwrap();
}

#[tokio::test]
async fn test_show_various_params() {
    let addr = start_gateway(test_config()).await;
    let client = connect(addr).await;

    for (param, expected) in [
        ("server_encoding", "UTF8"),
        ("client_encoding", "UTF8"),
        ("integer_datetimes", "on"),
        ("datestyle", "ISO, MDY"),
        ("timezone", "UTC"),
        ("intervalstyle", "postgres"),
        ("max_identifier_length", "63"),
        ("is_superuser", "on"),
    ] {
        let query = format!("SHOW {}", param);
        let rows = extract_rows(client.simple_query(&query).await.unwrap());
        let val = rows[0].get(0).unwrap();
        assert_eq!(
            val, expected,
            "SHOW {} returned '{}', expected '{}'",
            param, val, expected
        );
    }
}

#[tokio::test]
async fn test_current_setting() {
    let addr = start_gateway(test_config()).await;
    let client = connect(addr).await;
    let rows = extract_rows(
        client
            .simple_query("SELECT current_setting('server_version_num')")
            .await
            .unwrap(),
    );
    let val = rows[0].get(0).unwrap();
    assert_eq!(val, "160006");
}

// ---------------------------------------------------------------------------
// Trino pass-through tests (need TRINO_HOST env var)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_select_one_via_trino() {
    let config = match trino_config() {
        Some(c) => c,
        None => {
            eprintln!("Skipping: TRINO_HOST not set");
            return;
        }
    };
    let addr = start_gateway(config).await;
    let client = connect(addr).await;
    let rows = extract_rows(client.simple_query("SELECT 1 AS num").await.unwrap());
    assert_eq!(rows.len(), 1);
}

#[tokio::test]
async fn test_trino_with_limit() {
    let config = match trino_config() {
        Some(c) => c,
        None => {
            eprintln!("Skipping: TRINO_HOST not set");
            return;
        }
    };
    let addr = start_gateway(config).await;
    let client = connect(addr).await;
    let rows = extract_rows(
        client
            .simple_query("SELECT name FROM nation LIMIT 5")
            .await
            .unwrap(),
    );
    assert_eq!(rows.len(), 5);
}

#[tokio::test]
async fn test_trino_count() {
    let config = match trino_config() {
        Some(c) => c,
        None => {
            eprintln!("Skipping: TRINO_HOST not set");
            return;
        }
    };
    let addr = start_gateway(config).await;
    let client = connect(addr).await;
    let rows = extract_rows(
        client
            .simple_query("SELECT count(*) FROM nation")
            .await
            .unwrap(),
    );
    assert_eq!(rows.len(), 1);
}

#[tokio::test]
async fn test_trino_join() {
    let config = match trino_config() {
        Some(c) => c,
        None => {
            eprintln!("Skipping: TRINO_HOST not set");
            return;
        }
    };
    let addr = start_gateway(config).await;
    let client = connect(addr).await;
    let rows = extract_rows(
        client
            .simple_query("SELECT n.name, r.name AS region FROM nation n JOIN region r ON n.regionkey = r.regionkey LIMIT 3")
            .await
            .unwrap(),
    );
    assert_eq!(rows.len(), 3);
}

#[tokio::test]
async fn test_trino_aggregation() {
    let config = match trino_config() {
        Some(c) => c,
        None => {
            eprintln!("Skipping: TRINO_HOST not set");
            return;
        }
    };
    let addr = start_gateway(config).await;
    let client = connect(addr).await;
    let rows = extract_rows(
        client
            .simple_query("SELECT r.name, count(*) as cnt FROM nation n JOIN region r ON n.regionkey = r.regionkey GROUP BY r.name ORDER BY r.name")
            .await
            .unwrap(),
    );
    assert!(!rows.is_empty());
}

#[tokio::test]
async fn test_trino_error_handling() {
    let config = match trino_config() {
        Some(c) => c,
        None => {
            eprintln!("Skipping: TRINO_HOST not set");
            return;
        }
    };
    let addr = start_gateway(config).await;
    let client = connect(addr).await;
    // Query a non-existent table -- should get an error, not a crash
    let result = client
        .simple_query("SELECT * FROM nonexistent_table_xyz")
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_pg_class_dynamic_catalog() {
    let config = match trino_config() {
        Some(c) => c,
        None => {
            eprintln!("Skipping: TRINO_HOST not set");
            return;
        }
    };
    let addr = start_gateway(config).await;
    let client = connect(addr).await;
    let rows = extract_rows(
        client
            .simple_query(
                "SELECT relname, relkind FROM pg_catalog.pg_class WHERE relkind IN ('r', 'v')",
            )
            .await
            .unwrap(),
    );
    assert!(!rows.is_empty(), "pg_class should return tables from Trino");
}

#[tokio::test]
async fn test_information_schema_passthrough() {
    let config = match trino_config() {
        Some(c) => c,
        None => {
            eprintln!("Skipping: TRINO_HOST not set");
            return;
        }
    };
    let addr = start_gateway(config).await;
    let client = connect(addr).await;
    let rows = extract_rows(
        client
            .simple_query("SELECT table_name FROM information_schema.tables LIMIT 5")
            .await
            .unwrap(),
    );
    assert!(!rows.is_empty());
}

#[tokio::test]
async fn test_multiple_queries_same_connection() {
    let config = match trino_config() {
        Some(c) => c,
        None => {
            eprintln!("Skipping: TRINO_HOST not set");
            return;
        }
    };
    let addr = start_gateway(config).await;
    let client = connect(addr).await;

    // Run multiple queries on the same connection
    client
        .batch_execute("SET extra_float_digits = 3")
        .await
        .unwrap();
    let rows = extract_rows(client.simple_query("SELECT version()").await.unwrap());
    assert_eq!(rows.len(), 1);
    let rows = extract_rows(client.simple_query("SELECT 1").await.unwrap());
    assert_eq!(rows.len(), 1);
    let rows = extract_rows(
        client
            .simple_query("SELECT name FROM nation LIMIT 2")
            .await
            .unwrap(),
    );
    assert_eq!(rows.len(), 2);
}
