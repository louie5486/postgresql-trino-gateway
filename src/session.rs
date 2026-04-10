// Copyright 2026 Stackable GmbH
// Licensed under the Open Software License version 3.0 (OSL-3.0).
// See LICENSE file in the project root for full license text.
use std::sync::{Arc, LazyLock};

use dashmap::DashMap;
use trino_rust_client::Client as TrinoClient;

use crate::config::Config;

/// Per-connection state stored in a global map.
/// Keyed by a unique connection ID string stored in pgwire's client metadata.
pub struct ConnectionState {
    pub trino_client: Arc<TrinoClient>,
    pub config: Arc<Config>,
}

static CONNECTIONS: LazyLock<DashMap<String, ConnectionState>> = LazyLock::new(DashMap::new);

const CONNECTION_ID_KEY: &str = "_conn_id";

/// Store connection state keyed by `conn_id`.
pub fn register_connection(conn_id: String, state: ConnectionState) {
    CONNECTIONS.insert(conn_id, state);
}

/// Get connection state by ID.
pub fn get_connection(
    conn_id: &str,
) -> Option<dashmap::mapref::one::Ref<'_, String, ConnectionState>> {
    CONNECTIONS.get(conn_id)
}

/// Remove connection state (call on disconnect/cleanup).
pub fn remove_connection(conn_id: &str) {
    CONNECTIONS.remove(conn_id);
}

/// The metadata key used to store the connection ID.
pub fn connection_id_key() -> &'static str {
    CONNECTION_ID_KEY
}
