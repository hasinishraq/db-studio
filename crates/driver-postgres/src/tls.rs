//! rustls configuration for each TLS mode.

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::crypto::{ring, verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme};
use std::sync::{Arc, Mutex};
use studio_db::{DbError, TlsConfig};
use studio_types::TlsMode;
use tokio_postgres_rustls::MakeRustlsConnect;

/// Certificates the server presented, leaf first, as DER.
pub(crate) type CapturedChain = Arc<Mutex<Vec<Vec<u8>>>>;

/// What a mode needs at connect time.
pub(crate) enum PgTls {
    /// Plain TCP.
    Plain,
    /// Encrypted.
    Rustls(MakeRustlsConnect),
}

fn tls_error(message: impl Into<String>) -> DbError {
    DbError::Tls(message.into())
}

fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>, DbError> {
    CertificateDer::pem_file_iter(path)
        .and_then(|certs| certs.collect::<Result<Vec<_>, _>>())
        .map_err(|e| tls_error(format!("cannot read the certificate file: {e}")))
}

fn load_key(path: &str) -> Result<PrivateKeyDer<'static>, DbError> {
    PrivateKeyDer::from_pem_file(path).map_err(|e| tls_error(format!("cannot read the key file: {e}")))
}

fn root_store(ca_path: Option<&str>) -> Result<RootCertStore, DbError> {
    let mut roots = RootCertStore::empty();
    roots.add_parsable_certificates(rustls_native_certs::load_native_certs().certs);
    if let Some(path) = ca_path {
        for cert in load_certs(path)? {
            roots
                .add(cert)
                .map_err(|e| tls_error(format!("invalid CA certificate: {e}")))?;
        }
    }
    Ok(roots)
}

/// Checks the chain like the wrapped verifier, optionally forgiving only a host name mismatch, and
/// records what the server presented.
#[derive(Debug)]
struct ChainVerifier {
    inner: Arc<WebPkiServerVerifier>,
    ignore_name: bool,
    captured: CapturedChain,
}

impl ServerCertVerifier for ChainVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        capture(&self.captured, end_entity, intermediates);
        match self
            .inner
            .verify_server_cert(end_entity, intermediates, server_name, ocsp, now)
        {
            Err(Error::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
            )) if self.ignore_name => Ok(ServerCertVerified::assertion()),
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// `Require` mode: encrypts without trusting the certificate. Handshake signatures are still
/// checked, so the peer must at least hold the key it presents.
#[derive(Debug)]
struct NoChainVerifier {
    provider: Arc<CryptoProvider>,
    captured: CapturedChain,
}

impl ServerCertVerifier for NoChainVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        capture(&self.captured, end_entity, intermediates);
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn capture(captured: &CapturedChain, leaf: &CertificateDer<'_>, rest: &[CertificateDer<'_>]) {
    if let Ok(mut chain) = captured.lock() {
        chain.clear();
        chain.push(leaf.as_ref().to_vec());
        chain.extend(rest.iter().map(|c| c.as_ref().to_vec()));
    }
}

fn verifier(
    tls: &TlsConfig,
    provider: &Arc<CryptoProvider>,
    captured: &CapturedChain,
) -> Result<Arc<dyn ServerCertVerifier>, DbError> {
    if tls.mode == TlsMode::Require {
        return Ok(Arc::new(NoChainVerifier {
            provider: provider.clone(),
            captured: captured.clone(),
        }));
    }
    let roots = root_store(tls.ca_path.as_deref())?;
    let inner = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .build()
        .map_err(|e| tls_error(format!("cannot build the certificate verifier: {e}")))?;
    Ok(Arc::new(ChainVerifier {
        inner,
        ignore_name: tls.mode == TlsMode::VerifyCa,
        captured: captured.clone(),
    }))
}

/// Builds the connector for a mode. The provider is explicit, so no process default is needed.
pub(crate) fn build(tls: &TlsConfig, captured: &CapturedChain) -> Result<PgTls, DbError> {
    if tls.mode == TlsMode::Disable {
        return Ok(PgTls::Plain);
    }
    let provider = Arc::new(ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| tls_error(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(verifier(tls, &provider, captured)?);
    let config = match (&tls.client_cert_path, &tls.client_key_path) {
        (Some(cert), Some(key)) => builder
            .with_client_auth_cert(load_certs(cert)?, load_key(key)?)
            .map_err(|e| tls_error(format!("invalid client certificate: {e}")))?,
        (None, None) => builder.with_no_client_auth(),
        _ => {
            return Err(tls_error(
                "a client certificate needs both a certificate and a key file",
            ))
        }
    };
    Ok(PgTls::Rustls(MakeRustlsConnect::new(config)))
}
