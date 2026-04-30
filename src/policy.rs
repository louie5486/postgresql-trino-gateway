// Copyright 2026 Stackable GmbH
// Licensed under the Open Software License version 3.0 (OSL-3.0).
// See LICENSE file in the project root for full license text.

//! Startup-time security-policy validation.
//!
//! `--auth` enables a cleartext-password challenge on each PG connection
//! (the only auth mechanism the gateway implements; SCRAM-SHA-256 is not a
//! practical fit because the gateway forwards the same credentials to Trino
//! as HTTP Basic auth, which needs the password in the clear). Cleartext on
//! a plaintext listener is unsafe on any network. We refuse to start that
//! configuration unless the listener is bound to a loopback address, where
//! the operator has explicitly opted into a dev-only setup.

use std::net::SocketAddr;

use anyhow::{Context, Result, bail};

use crate::config::Config;

/// Result of inspecting the (auth, TLS, listen-addr) triple. Returned for
/// logging/diagnostic use; the policy decision is enforced by `validate`'s
/// `Result`.
#[derive(Debug, PartialEq, Eq)]
pub enum AuthPosture {
    /// `--auth` is off. The gateway connects to Trino with `--trino-user`
    /// and no per-client credentials.
    Disabled,
    /// `--auth` is on and the listening socket is TLS-terminated, so
    /// passwords cross the wire encrypted.
    CleartextOverTls,
    /// `--auth` is on, no TLS, but the listener is a loopback address.
    /// Acceptable for local development only.
    CleartextLoopback,
}

/// Validate the gateway's security policy and emit a startup log line. The
/// returned `AuthPosture` is informational; the `Err` arm is the only
/// blocking outcome.
pub fn validate(config: &Config) -> Result<AuthPosture> {
    let posture = classify(config)?;
    match posture {
        AuthPosture::Disabled => {
            tracing::info!("auth disabled — connecting to Trino as --trino-user");
        }
        AuthPosture::CleartextOverTls => {
            tracing::info!(
                "auth enabled — cleartext password over TLS (forwarded to Trino as HTTP Basic)"
            );
        }
        AuthPosture::CleartextLoopback => {
            tracing::warn!(
                addr = %config.listen_addr,
                "auth enabled on a plaintext loopback listener — dev only, NOT for production. \
                 Configure --tls-cert and --tls-key for any non-loopback deployment."
            );
        }
    }
    Ok(posture)
}

/// Pure classification helper, separated from logging so it can be tested
/// without a tracing subscriber.
pub fn classify(config: &Config) -> Result<AuthPosture> {
    if !config.auth {
        return Ok(AuthPosture::Disabled);
    }

    let has_tls = config.tls_cert.is_some() && config.tls_key.is_some();
    if has_tls {
        return Ok(AuthPosture::CleartextOverTls);
    }

    let addr: SocketAddr = config
        .listen_addr
        .parse()
        .with_context(|| format!("invalid --listen-addr: {}", config.listen_addr))?;
    if addr.ip().is_loopback() {
        return Ok(AuthPosture::CleartextLoopback);
    }

    bail!(
        "--auth on non-loopback bind ({}) requires TLS. \
         Set --tls-cert and --tls-key, or bind to 127.0.0.1 / [::1] for dev.",
        config.listen_addr
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn cfg(auth: bool, listen: &str, tls: bool) -> Config {
        Config {
            listen_addr: listen.to_owned(),
            tls_cert: tls.then(|| PathBuf::from("/tmp/cert")),
            tls_key: tls.then(|| PathBuf::from("/tmp/key")),
            trino_host: "h".to_owned(),
            trino_port: 8080,
            trino_catalog: "c".to_owned(),
            trino_schema: "s".to_owned(),
            trino_user: "u".to_owned(),
            trino_ssl: false,
            trino_ssl_insecure: false,
            auth,
        }
    }

    #[test]
    fn auth_disabled_is_always_ok() {
        assert_eq!(
            classify(&cfg(false, "0.0.0.0:5432", false)).unwrap(),
            AuthPosture::Disabled
        );
        assert_eq!(
            classify(&cfg(false, "127.0.0.1:5432", false)).unwrap(),
            AuthPosture::Disabled
        );
    }

    #[test]
    fn auth_with_tls_passes_on_any_address() {
        assert_eq!(
            classify(&cfg(true, "0.0.0.0:5432", true)).unwrap(),
            AuthPosture::CleartextOverTls
        );
        assert_eq!(
            classify(&cfg(true, "127.0.0.1:5432", true)).unwrap(),
            AuthPosture::CleartextOverTls
        );
        assert_eq!(
            classify(&cfg(true, "[::]:5432", true)).unwrap(),
            AuthPosture::CleartextOverTls
        );
    }

    #[test]
    fn auth_without_tls_on_loopback_warns_but_passes() {
        assert_eq!(
            classify(&cfg(true, "127.0.0.1:5432", false)).unwrap(),
            AuthPosture::CleartextLoopback
        );
        assert_eq!(
            classify(&cfg(true, "[::1]:5432", false)).unwrap(),
            AuthPosture::CleartextLoopback
        );
    }

    #[test]
    fn auth_without_tls_on_non_loopback_is_refused() {
        let err = classify(&cfg(true, "0.0.0.0:5432", false)).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("requires TLS"), "actionable message: {msg}");
        assert!(msg.contains("0.0.0.0:5432"), "echo bind addr: {msg}");
    }

    #[test]
    fn auth_without_tls_on_public_ip_is_refused() {
        let err = classify(&cfg(true, "10.20.30.40:5432", false)).unwrap_err();
        assert!(format!("{err:#}").contains("requires TLS"));
    }

    #[test]
    fn invalid_listen_addr_with_auth_no_tls_surfaces_parse_error() {
        let err = classify(&cfg(true, "not-an-address", false)).unwrap_err();
        assert!(format!("{err:#}").contains("invalid --listen-addr"));
    }

    /// Asymmetric tls_cert without tls_key is treated as no-TLS by the
    /// has_tls check. A non-loopback bind in that state is refused, which
    /// matches the main.rs warning that already fires.
    #[test]
    fn asymmetric_tls_flags_are_treated_as_no_tls() {
        let mut c = cfg(true, "0.0.0.0:5432", false);
        c.tls_cert = Some(PathBuf::from("/tmp/cert"));
        c.tls_key = None;
        assert!(classify(&c).is_err());
    }
}
