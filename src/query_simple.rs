use std::fmt::Debug;
use std::sync::Arc;

use async_trait::async_trait;
use futures::Sink;
use pgwire::api::results::{DataRowEncoder, FieldFormat, FieldInfo, QueryResponse, Response};
use pgwire::api::store::PortalStore;
use pgwire::api::{ClientInfo, ClientPortalStore, Type};
use pgwire::error::{PgWireError, PgWireResult};
use pgwire::messages::PgWireBackendMessage;

use pgwire::api::query::SimpleQueryHandler;

/// Handles simple query protocol messages.
#[derive(Debug)]
pub struct GatewayQueryHandler;

#[async_trait]
impl SimpleQueryHandler for GatewayQueryHandler {
    async fn do_query<C>(&self, _client: &mut C, _query: &str) -> PgWireResult<Vec<Response>>
    where
        C: ClientInfo + ClientPortalStore + Sink<PgWireBackendMessage> + Unpin + Send + Sync,
        C::PortalStore: PortalStore,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        let schema = Arc::new(vec![FieldInfo::new(
            "?column?".to_owned(),
            None,
            None,
            Type::INT4,
            FieldFormat::Text,
        )]);

        let mut encoder = DataRowEncoder::new(schema.clone());
        encoder.encode_field(&1i32)?;
        let row = encoder.take_row();

        let stream = futures::stream::iter(vec![Ok(row)]);
        Ok(vec![Response::Query(QueryResponse::new(schema, stream))])
    }
}
