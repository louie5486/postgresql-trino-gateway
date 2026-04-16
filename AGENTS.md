# PostgreSQL-Trino Gateway

Rust service speaking PostgreSQL wire protocol on the frontend, forwarding queries to Trino's REST API on the backend. Enables Power BI Report Server to use Trino as a DirectQuery source.

## Architecture

- **pgwire frontend**: Accepts PostgreSQL client connections (psql, Npgsql, Power BI)
- **Intercept layer**: Handles SET, SHOW, BEGIN/COMMIT, pg_catalog queries locally
- **SQL rewriter**: Transforms PG-dialect SQL to Trino-compatible SQL (::cast, ILIKE, function names)
- **Trino backend**: Forwards queries via REST API, streams results back as PG wire protocol rows
- **Catalog emulation**: Fakes pg_type, pg_class, pg_attribute from Trino's information_schema

## Project Structure

- `gateway/src/` — main binary crate
  - `main.rs` — CLI arg parsing, TCP listener
  - `config.rs` — `Config` struct (clap derive)
  - `startup.rs` — PG connection startup, server params, Trino client creation
  - `handler.rs` — `PgWireServerHandlers` factory
  - `query_simple.rs` — simple query protocol handler
  - `query_extended.rs` — extended query protocol (Parse/Bind/Execute)
  - `intercept.rs` — SET, SHOW, transaction, server function interception
  - `catalog/` — pg_catalog emulation (pg_type, pg_class, pg_attribute, stubs)
  - `rewrite/` — SQL rewriting (casts, predicates, functions)
  - `types.rs` — Trino-to-PG type mapping and value encoding
  - `trino_stream.rs` — streaming bridge (poll Trino, yield PG DataRow)
  - `error_mapping.rs` — Trino errors to PG SQLSTATE codes
- `gateway/tests/integration_test.rs` — data-driven integration tests

## Build & Test

```bash
cargo build --manifest-path gateway/Cargo.toml
cargo test --manifest-path gateway/Cargo.toml                    # unit tests only
cargo clippy --manifest-path gateway/Cargo.toml                  # lint check
cargo fmt --manifest-path gateway/Cargo.toml --check             # format check

# With Trino (read-only tests):
TRINO_HOST=... TRINO_PORT=... TRINO_SSL=true TRINO_SSL_INSECURE=true \
  TRINO_CATALOG=tpch TRINO_SCHEMA=sf1 \
  cargo test --manifest-path gateway/Cargo.toml

# With writable catalog (DDL tests):
... TRINO_WRITE_CATALOG=memory TRINO_WRITE_SCHEMA=default \
  cargo test --manifest-path gateway/Cargo.toml
```

## Quality Rules

These are non-negotiable. Fewer features done well beats more features done poorly.

### Before every commit

- **Zero warnings**: `cargo build` and `cargo clippy` must produce no warnings. Do not use `#[allow(dead_code)]` to suppress warnings — remove unused code instead.
- **Formatting**: `cargo fmt --check` must pass.
- **Tests**: `cargo test` must pass. All new functionality must have tests.
- **No dead code**: Remove unused fields, methods, imports, and structs rather than suppressing warnings.

### Code quality (non-negotiable)

- **Readability over cleverness**: This code is written by AI but maintained by humans. Use straightforward, well-known Rust idioms. If a reviewer has to stop and think about what a block of code does, it's too clever. Prefer boring and obvious.
- **No duplication**: Copying a pattern once is acceptable. Three times means extract a function, trait, or helper. Audit for shared patterns and consolidate them.
- **Sound architecture**: Every module should have a clear single responsibility. Dependencies flow one way. No circular imports. If you're passing 5+ arguments to a function, consider whether the design is right.
- **Error messages must be actionable**: Every error a user or operator sees should tell them what went wrong AND what to do about it. "connection refused" is bad. "Failed to connect to Trino at host:port — is Trino running?" is good.

### Security (non-negotiable)

This gateway runs at highly sensitive customer sites. Security is not optional.

- **No panics in production paths**: Use `Result` and propagate errors. `unwrap()` is only acceptable in tests, static initialization, and cases where the invariant is provably guaranteed. Document why with a comment.
- **No SQL injection**: Never interpolate user input into SQL strings. Use parameterized queries or properly escape values. The SQL rewriter must not introduce injection vectors.
- **No credential leaking**: Never log passwords, tokens, or connection strings. Trino credentials must not appear in error messages or debug output.
- **Input validation at boundaries**: Validate and sanitize all input from PG clients before processing. Malformed wire protocol messages must not crash the gateway.
- **Fail closed**: If something unexpected happens, return an error to the client rather than proceeding with potentially corrupted state.
- **Dependency hygiene**: Minimize dependencies. Audit new crates before adding them. Prefer well-maintained, widely-used crates.

### Maintainability (non-negotiable)

- **Self-documenting code**: Names should make comments unnecessary. If you need a comment to explain *what* the code does, rename things. Comments should explain *why*, not *what*.
- **Small functions**: If a function doesn't fit on one screen, it probably does too much. Extract helpers.
- **Consistent patterns**: If one catalog handler works a certain way, they all should. If one rewrite visitor has a certain structure, they all should. Inconsistency is a maintenance burden.
- **Tests as documentation**: A reader should be able to understand the behavior of a module by reading its tests. Test names should describe the scenario, not the implementation.

## Debugging protocol issues

For issues where a client (e.g. Power BI, Npgsql, psql) fails against the gateway but works against real PostgreSQL, enable protocol-level tracing:

```bash
RUST_LOG=postgresql_trino_gateway=trace cargo run --manifest-path gateway/Cargo.toml -- ...
```

Trace output shows, per connection:
- Startup message and auth flow (passwords redacted)
- Every simple-query and extended-query message with the SQL text
- Which intercept branch matched (SET, SHOW, pg_catalog, info_schema, …) or whether the query was forwarded to Trino
- The rewritten SQL sent to Trino (if any rewrite applied)
- Trino's response shape (column count, row count) — never row contents

Row contents are NEVER logged to avoid leaking customer data. If you need to see values, use a pre-production Trino catalog with synthetic data.

The trace output is structured — pipe through `jq` or grep for `conn_id=` to filter by connection.

## Conventions

### Code Style

- Services are stored per-connection in pgwire's `SessionExtensions`
- Use `async_stream::stream!` for streaming bridges
- Use text wire format (format code 0) for all PG responses
- Catalog queries return pre-built static responses, not parsed/executed SQL
- SQL rewriting uses `sqlparser-rs` with `PostgreSqlDialect`, falls back to passthrough on parse failure
- Integration tests are data-driven: `(name, sql, Check)` tuples with shared fixtures

### Adding a new intercepted query

1. Add pattern detection in `intercept.rs` (or `catalog/mod.rs` for pg_catalog tables)
2. Build response using `single_text_response()` or `build_response()` helpers
3. Add test case to the appropriate test function

### Adding a new SQL rewrite

1. Add visitor or AST walking logic in the appropriate `rewrite/` submodule
2. Add unit test in `rewrite/mod.rs`
3. Add integration test case in `integration_test.rs`
