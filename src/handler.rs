use std::sync::Arc;

use pgwire::api::PgWireServerHandlers;
use pgwire::api::auth::StartupHandler;
use pgwire::api::query::SimpleQueryHandler;

use crate::query_simple::GatewayQueryHandler;
use crate::startup::GatewayStartupHandler;

/// Factory that provides handler implementations to pgwire.
pub struct GatewayHandlerFactory {
    pub startup: Arc<GatewayStartupHandler>,
    pub query: Arc<GatewayQueryHandler>,
}

impl PgWireServerHandlers for GatewayHandlerFactory {
    fn startup_handler(&self) -> Arc<impl StartupHandler> {
        self.startup.clone()
    }

    fn simple_query_handler(&self) -> Arc<impl SimpleQueryHandler> {
        self.query.clone()
    }
}
