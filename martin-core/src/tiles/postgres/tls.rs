use std::fs::File;
use std::io;
use std::io::BufReader;
use std::path::PathBuf;
use std::str::FromStr as _;
use std::sync::{Arc, LazyLock};

use deadpool_postgres::tokio_postgres::config::SslMode;
use deadpool_postgres::tokio_postgres::{Config, NoTls};
use regex::Regex;
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{aws_lc_rs, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, Error, SignatureScheme};
use rustls_native_certs::load_native_certs;
use rustls_pemfile::Item::Pkcs1Key;
use tokio_postgres_rustls::MakeRustlsConnect;
use tracing::{info, warn};

use crate::tiles::postgres::PostgresError::{
    BadConnectionString, CannotLoadRoots, CannotOpenCert, CannotParseCert, CannotUseClientKey,
    InvalidPrivateKey, UnknownSslMode,
};
use crate::tiles::postgres::PostgresResult;

/// Pool TLS settings (`NoTls` or rustls), cloned for query cancel.
///
/// Cancel opens a new connection, so it needs the same connector the pool uses.
#[derive(Clone)]
pub(crate) enum PgTlsConnector {
    NoTls(NoTls),
    Rustls(MakeRustlsConnect),
}

impl Default for PgTlsConnector {
    fn default() -> Self {
        Self::NoTls(NoTls)
    }
}

/// A temporary workaround for <https://github.com/sfackler/rust-postgres/pull/988>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslModeOverride {
    Unmodified(SslMode),
    VerifyCa,
    VerifyFull,
}

/// Special treatment for sslmode=verify-ca & sslmode=verify-full - if found, replace them with sslmode=require
pub fn parse_conn_str(conn_str: &str) -> PostgresResult<(Config, SslModeOverride)> {
    let mut mode = SslModeOverride::Unmodified(SslMode::Disable);

    let exp = r"(?P<before>(^|\?|&| )sslmode=)(?P<mode>verify-(ca|full))(?P<after>$|&| )";
    let re = Regex::new(exp).expect("the regex is valid");
    let pg_cfg = if let Some(captures) = re.captures(conn_str) {
        let captured_value = &captures["mode"];
        mode = match captured_value {
            "verify-ca" => SslModeOverride::VerifyCa,
            "verify-full" => SslModeOverride::VerifyFull,
            _ => unreachable!(),
        };
        let conn_str = re.replace(conn_str, "${before}require${after}");
        Config::from_str(conn_str.as_ref())
    } else {
        Config::from_str(conn_str)
    };
    let mut pg_cfg = pg_cfg.map_err(|e| BadConnectionString(e, conn_str.into()))?;
    if let SslModeOverride::Unmodified(_) = mode {
        mode = SslModeOverride::Unmodified(pg_cfg.get_ssl_mode());
    }
    let crate_ver = env!("CARGO_PKG_VERSION");
    if pg_cfg.get_application_name().is_none() {
        let pid = std::process::id();
        pg_cfg.application_name(format!("Martin v{crate_ver} - pid={pid}"));
    }
    Ok((pg_cfg, mode))
}

#[derive(Debug)]
struct NoCertificateVerification;

impl ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
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
            &aws_lc_rs::default_provider().signature_verification_algorithms,
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
            &aws_lc_rs::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        aws_lc_rs::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Verifies the certificate chain against the configured roots but accepts a hostname mismatch.
///
/// This implements the semantics of `sslmode=verify-ca`, which checks that the server certificate
/// is signed by a trusted CA while skipping the hostname check performed by `verify-full`.
/// See <https://www.postgresql.org/docs/current/libpq-ssl.html#LIBQ-SSL-CERTIFICATES>.
#[derive(Debug)]
struct NoHostnameVerification(Arc<WebPkiServerVerifier>);

impl ServerCertVerifier for NoHostnameVerification {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        match self
            .0
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
        {
            Err(Error::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
            )) => Ok(ServerCertVerified::assertion()),
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.0.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.0.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_verify_schemes()
    }
}

fn read_certs(file: &PathBuf) -> PostgresResult<Vec<CertificateDer<'static>>> {
    rustls_pemfile::certs(&mut cert_reader(file)?)
        .collect::<Result<Vec<_>, io::Error>>()
        .map_err(|e| CannotParseCert(e, file.clone()))
}

fn cert_reader(file: &PathBuf) -> PostgresResult<BufReader<File>> {
    Ok(BufReader::new(
        File::open(file).map_err(|e| CannotOpenCert(e, file.clone()))?,
    ))
}

/// Ensure a rustls crypto provider is installed (no-op if one already is).
///
/// Needed when `make_connector` runs outside normal martin startup (e.g. tests),
/// which does not call `martin::init_aws_lc_tls`.
fn ensure_rustls_provider() {
    static INIT_TLS: LazyLock<()> = LazyLock::new(|| {
        let _ = aws_lc_rs::default_provider().install_default();
    });
    *INIT_TLS;
}

pub fn make_connector(
    ssl_cert: Option<&PathBuf>,
    ssl_key: Option<&PathBuf>,
    ssl_root_cert: Option<&PathBuf>,
    ssl_mode: SslModeOverride,
) -> PostgresResult<MakeRustlsConnect> {
    ensure_rustls_provider();

    let (verify_ca, verify_hostname) = match ssl_mode {
        SslModeOverride::Unmodified(mode) => match mode {
            SslMode::Disable | SslMode::Prefer => (false, false),
            SslMode::Require => match ssl_root_cert {
                // If a root CA file exists, the behavior of sslmode=require will be the same as
                // that of verify-ca, meaning the server certificate is validated against the CA.
                // For more details, check out the note about backwards compatibility in
                // https://postgresql.org/docs/current/libpq-ssl.html#LIBQ-SSL-CERTIFICATES
                // See also notes in
                // https://github.com/sfu-db/connector-x/blob/b26f3b73714259dc55010f2233e663b64d24f1b1/connectorx/src/sources/postgres/connection.rs#L25
                Some(_) => (true, false),
                None => (false, false),
            },
            _ => return Err(UnknownSslMode(mode)),
        },
        SslModeOverride::VerifyCa => (true, false),
        SslModeOverride::VerifyFull => (true, true),
    };

    let mut roots = rustls::RootCertStore::empty();

    if let Some(file) = &ssl_root_cert {
        for cert in read_certs(file)? {
            roots.add(cert)?;
        }
        info!("Using {} as a root certificate", file.display());
    }

    if verify_ca || ssl_root_cert.is_some() || ssl_cert.is_some() {
        let certs = load_native_certs();
        if !certs.errors.is_empty() {
            return Err(CannotLoadRoots(certs.errors));
        }
        for cert in certs.certs {
            roots.add(cert)?;
        }
    }

    let roots = Arc::new(roots);
    let builder = rustls::ClientConfig::builder().with_root_certificates(roots.clone());

    let mut builder = if let (Some(cert), Some(key)) = (ssl_cert, ssl_key) {
        match rustls_pemfile::read_one(&mut cert_reader(key)?) {
            Ok(Some(Pkcs1Key(rsa_key))) => builder
                .with_client_auth_cert(read_certs(cert)?, rsa_key.into())
                .map_err(|e| CannotUseClientKey {
                    source: e,
                    cert: cert.clone(),
                    key: key.clone(),
                })?,
            Ok(_) => return Err(InvalidPrivateKey(key.clone())),
            Err(e) => return Err(CannotParseCert(e, key.clone())),
        }
    } else {
        if ssl_key.is_some() || ssl_key.is_some() {
            warn!(
                "SSL client certificate and key files must be set to use client certificate with Postgres. Only one of them was set."
            );
        }
        builder.with_no_client_auth()
    };

    if !verify_ca {
        builder
            .dangerous()
            .set_certificate_verifier(Arc::new(NoCertificateVerification));
    } else if !verify_hostname {
        let verifier = WebPkiServerVerifier::builder(roots).build()?;
        builder
            .dangerous()
            .set_certificate_verifier(Arc::new(NoHostnameVerification(verifier)));
    }

    Ok(MakeRustlsConnect::new(builder))
}

#[cfg(test)]
mod tests {
    use deadpool_postgres::tokio_postgres::config::Host;

    use super::*;

    #[test]
    fn test_parse_conn_str() {
        let (cfg, mode) = parse_conn_str("postgres://user:password@localhost:5432/dbname").unwrap();
        assert_eq!(cfg.get_hosts(), &vec![Host::Tcp("localhost".to_string())]);
        assert_eq!(cfg.get_ports(), &vec![5432]);
        assert_eq!(cfg.get_user(), Some("user"));
        assert_eq!(cfg.get_dbname(), Some("dbname"));
        assert_eq!(cfg.get_password(), Some(b"password".as_ref()));
        assert_eq!(cfg.get_ssl_mode(), SslMode::Prefer);
        assert_eq!(mode, SslModeOverride::Unmodified(SslMode::Prefer));

        let (cfg, mode) = parse_conn_str("postgres://localhost:5432/db?sslmode=verify-ca").unwrap();
        assert_eq!(cfg.get_ssl_mode(), SslMode::Require);
        assert_eq!(mode, SslModeOverride::VerifyCa);

        let conn = "postgres://localhost:5432?sslmode=verify-full";
        let (cfg, mode) = parse_conn_str(conn).unwrap();
        assert_eq!(cfg.get_ssl_mode(), SslMode::Require);
        assert_eq!(mode, SslModeOverride::VerifyFull);

        let conn = "postgres://localhost:5432?sslmode=verify-full&connect_timeout=5";
        let (cfg, mode) = parse_conn_str(conn).unwrap();
        assert_eq!(cfg.get_ssl_mode(), SslMode::Require);
        assert_eq!(mode, SslModeOverride::VerifyFull);

        let conn = "host=localhost sslmode=verify-full";
        let (cfg, mode) = parse_conn_str(conn).unwrap();
        assert_eq!(cfg.get_ssl_mode(), SslMode::Require);
        assert_eq!(mode, SslModeOverride::VerifyFull);

        let conn = "sslmode=verify-ca host=localhost";
        let (cfg, mode) = parse_conn_str(conn).unwrap();
        assert_eq!(cfg.get_ssl_mode(), SslMode::Require);
        assert_eq!(mode, SslModeOverride::VerifyCa);
    }
}
