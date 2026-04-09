use clap::Parser;

/// PostgreSQL-to-Trino gateway configuration.
#[derive(Debug, Parser)]
#[command(name = "postgresql-trino-gateway")]
pub struct Config {
    /// Address to listen on for PostgreSQL connections.
    #[arg(long, default_value = "127.0.0.1:5432")]
    pub listen_addr: String,

    /// Trino host to connect to.
    #[arg(long, default_value = "localhost")]
    pub trino_host: String,

    /// Trino port to connect to.
    #[arg(long, default_value_t = 8080)]
    pub trino_port: u16,

    /// Trino catalog to use.
    #[arg(long, default_value = "memory")]
    pub trino_catalog: String,

    /// Trino schema to use.
    #[arg(long, default_value = "default")]
    pub trino_schema: String,

    /// Trino user to authenticate as.
    #[arg(long, default_value = "trino")]
    pub trino_user: String,
}
