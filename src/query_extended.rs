use std::fmt::Debug;
use std::sync::Arc;

use async_trait::async_trait;
use futures::Sink;
use pgwire::api::portal::Portal;
use pgwire::api::query::ExtendedQueryHandler;
use pgwire::api::results::{DescribePortalResponse, DescribeStatementResponse, Response};
use pgwire::api::stmt::{NoopQueryParser, StoredStatement};
use pgwire::api::store::PortalStore;
use pgwire::api::{ClientInfo, ClientPortalStore, Type};
use pgwire::error::{PgWireError, PgWireResult};
use pgwire::messages::PgWireBackendMessage;
use trino_rust_client::Client as TrinoClient;

use crate::config::Config;
use crate::query_pipeline::process_query;

/// Handles the extended query protocol (Parse/Bind/Describe/Execute).
///
/// Npgsql and other drivers use this for all parameterized queries. Power BI
/// DirectQuery generates queries like `SELECT "col" FROM "table" WHERE "col" = $1::text`.
#[derive(Debug)]
pub struct GatewayExtendedQueryHandler;

#[async_trait]
impl ExtendedQueryHandler for GatewayExtendedQueryHandler {
    type Statement = String;
    type QueryParser = NoopQueryParser;

    fn query_parser(&self) -> Arc<Self::QueryParser> {
        Arc::new(NoopQueryParser)
    }

    async fn do_query<C>(
        &self,
        client: &mut C,
        portal: &Portal<Self::Statement>,
        _max_rows: usize,
    ) -> PgWireResult<Response>
    where
        C: ClientInfo + ClientPortalStore + Sink<PgWireBackendMessage> + Unpin + Send + Sync,
        C::PortalStore: PortalStore<Statement = Self::Statement>,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        let query = &portal.statement.statement;
        tracing::debug!(query, "Extended query execute");

        let trino_client: Arc<TrinoClient> = client
            .session_extensions()
            .get::<TrinoClient>()
            .ok_or_else(|| PgWireError::ApiError("No Trino client in session".into()))?;

        let config: Arc<Config> = client
            .session_extensions()
            .get::<Config>()
            .ok_or_else(|| PgWireError::ApiError("No Config in session".into()))?;

        let responses = process_query(query, &trino_client, &config).await?;
        responses
            .into_iter()
            .next()
            .ok_or_else(|| PgWireError::ApiError("Empty pipeline response".into()))
    }

    async fn do_describe_statement<C>(
        &self,
        _client: &mut C,
        stmt: &StoredStatement<Self::Statement>,
    ) -> PgWireResult<DescribeStatementResponse>
    where
        C: ClientInfo + ClientPortalStore + Sink<PgWireBackendMessage> + Unpin + Send + Sync,
        C::PortalStore: PortalStore<Statement = Self::Statement>,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        // Report all parameters as TEXT. Trino handles the actual type coercion,
        // and Npgsql is happy as long as it gets valid OIDs back.
        let param_types = stmt
            .parameter_types
            .iter()
            .map(|t| t.clone().unwrap_or(Type::TEXT))
            .collect();

        // We cannot know the result columns without executing the query, so
        // return an empty field list. The client will get the real schema from
        // the RowDescription sent during Execute.
        Ok(DescribeStatementResponse::new(param_types, vec![]))
    }

    async fn do_describe_portal<C>(
        &self,
        client: &mut C,
        portal: &Portal<Self::Statement>,
    ) -> PgWireResult<DescribePortalResponse>
    where
        C: ClientInfo + ClientPortalStore + Sink<PgWireBackendMessage> + Unpin + Send + Sync,
        C::PortalStore: PortalStore<Statement = Self::Statement>,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        // The JDBC/ODBC driver needs the column schema from Describe Portal
        // BEFORE Execute sends DataRow messages. Without this, clients get
        // "Received resultset tuples, but no field structure for them".
        //
        // We run the full query pipeline to get the schema. This means the
        // query executes during Describe and again during Execute — a double
        // execution. This is acceptable because:
        // 1. Intercepted/catalog queries are cheap (no Trino round-trip)
        // 2. For Trino queries, the alternative (not supporting Describe Portal)
        //    makes JDBC/ODBC clients completely unusable
        let query = &portal.statement.statement;
        tracing::debug!(query, "Extended query describe portal");

        let trino_client: Arc<TrinoClient> = client
            .session_extensions()
            .get::<TrinoClient>()
            .ok_or_else(|| PgWireError::ApiError("No Trino client in session".into()))?;

        let config: Arc<Config> = client
            .session_extensions()
            .get::<Config>()
            .ok_or_else(|| PgWireError::ApiError("No Config in session".into()))?;

        let responses = process_query(query, &trino_client, &config).await?;
        let fields = match responses.into_iter().next() {
            Some(Response::Query(qr)) => qr.row_schema.as_ref().clone(),
            _ => vec![], // DDL/DML — no columns, NoData is correct
        };

        Ok(DescribePortalResponse::new(fields))
    }
}
