// Copyright 2026 Stackable GmbH
// Licensed under the Open Software License version 3.0 (OSL-3.0).
// See LICENSE file in the project root for full license text.

//! TLS termination for the PostgreSQL listening socket.
//!
//! Loads a PEM-encoded certificate chain and private key from disk and
//! returns a `TlsAcceptor` ready to hand to `pgwire::tokio::process_socket`.
//! The crypto provider (`aws-lc-rs`, pulled in by pgwire's default features)
//! must be installed once at process startup before any acceptor is built —
//! `install_default_crypto_provider()` does that idempotently.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use pgwire::tokio::TlsAcceptor;
use pgwire::tokio::tokio_rustls::rustls::ServerConfig;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

/// Install the `aws-lc-rs` crypto provider as the rustls default. Idempotent:
/// repeated calls (or a previous installation by another component) are
/// silently treated as success.
pub fn install_default_crypto_provider() {
    use pgwire::tokio::tokio_rustls::rustls::crypto::aws_lc_rs;
    let _ = aws_lc_rs::default_provider().install_default();
}

/// Build a `TlsAcceptor` from PEM-encoded certificate chain and private key
/// files on disk.
pub fn build_acceptor(cert_path: &Path, key_path: &Path) -> Result<TlsAcceptor> {
    let certs = load_certs(cert_path)?;
    let key = load_key(key_path)?;

    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .with_context(|| {
            format!(
                "TLS keypair invalid (cert {}, key {})",
                cert_path.display(),
                key_path.display()
            )
        })?;

    Ok(TlsAcceptor::from(Arc::new(cfg)))
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let file = File::open(path)
        .with_context(|| format!("opening TLS certificate file {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("reading TLS certificate PEM {}", path.display()))?;
    if certs.is_empty() {
        return Err(anyhow!(
            "no PEM certificates found in {}",
            path.display()
        ));
    }
    Ok(certs)
}

fn load_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let file =
        File::open(path).with_context(|| format!("opening TLS key file {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let key = rustls_pemfile::private_key(&mut reader)
        .with_context(|| format!("reading TLS key PEM {}", path.display()))?
        .ok_or_else(|| anyhow!("no PEM private key found in {}", path.display()))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// `TlsAcceptor` doesn't implement `Debug`, so `unwrap_err` on the
    /// `Result<TlsAcceptor, _>` doesn't compile. Match instead.
    fn err_msg(r: Result<TlsAcceptor>) -> String {
        match r {
            Ok(_) => panic!("expected error"),
            Err(e) => format!("{e:#}"),
        }
    }

    #[test]
    fn build_acceptor_rejects_missing_cert_file() {
        let msg = err_msg(build_acceptor(
            Path::new("/nonexistent/cert.pem"),
            Path::new("/nonexistent/key.pem"),
        ));
        assert!(
            msg.contains("certificate"),
            "error should mention certificate: {msg}"
        );
    }

    #[test]
    fn build_acceptor_rejects_empty_cert_file() {
        let mut cert = tempfile::NamedTempFile::new().unwrap();
        let mut key = tempfile::NamedTempFile::new().unwrap();
        cert.write_all(b"").unwrap();
        key.write_all(b"").unwrap();
        let msg = err_msg(build_acceptor(cert.path(), key.path()));
        assert!(
            msg.contains("no PEM certificates"),
            "error should mention missing PEM certs: {msg}"
        );
    }

    #[test]
    fn build_acceptor_rejects_garbage_cert() {
        let mut cert = tempfile::NamedTempFile::new().unwrap();
        let mut key = tempfile::NamedTempFile::new().unwrap();
        cert.write_all(b"this is not a PEM file").unwrap();
        key.write_all(b"neither is this").unwrap();
        let msg = err_msg(build_acceptor(cert.path(), key.path()));
        // Either "no PEM certificates" (if the parser silently skips garbage)
        // or a parse error from rustls_pemfile.
        assert!(
            msg.contains("PEM") || msg.contains("certificate"),
            "error should mention PEM/certificate parsing: {msg}"
        );
    }

    #[test]
    fn install_provider_is_idempotent() {
        // Repeat calls must not panic.
        install_default_crypto_provider();
        install_default_crypto_provider();
        install_default_crypto_provider();
    }
}
