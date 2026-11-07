//! TLS options per mode.

use mysql_async::{ClientIdentity, SslOpts};
use std::path::PathBuf;
use studio_db::{DbError, TlsConfig};
use studio_types::TlsMode;

/// Builds the options, or `None` for plain TCP. Verification stays on unless the mode says otherwise.
pub(crate) fn ssl_opts(tls: &TlsConfig) -> Result<Option<SslOpts>, DbError> {
    if tls.mode == TlsMode::Disable {
        return Ok(None);
    }
    let mut opts = SslOpts::default();
    if let Some(ca) = &tls.ca_path {
        opts = opts.with_root_certs(vec![PathBuf::from(ca).into()]);
    }
    opts = match tls.mode {
        TlsMode::Require => opts.with_danger_accept_invalid_certs(true),
        TlsMode::VerifyCa => opts.with_danger_skip_domain_validation(true),
        TlsMode::VerifyFull | TlsMode::Disable => opts,
    };
    if let Some(name) = &tls.server_name {
        opts = opts.with_danger_tls_hostname_override(Some(name.clone()));
    }
    match (&tls.client_cert_path, &tls.client_key_path) {
        (Some(cert), Some(key)) => {
            let identity = ClientIdentity::new(PathBuf::from(cert).into(), PathBuf::from(key).into());
            Ok(Some(opts.with_client_identity(Some(identity))))
        }
        (None, None) => Ok(Some(opts)),
        _ => Err(DbError::Tls(
            "a client certificate needs both a certificate and a key file".to_owned(),
        )),
    }
}
