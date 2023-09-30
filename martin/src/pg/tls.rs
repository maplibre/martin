use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::str::FromStr;

use deadpool_postgres::tokio_postgres::config::SslMode;
use deadpool_postgres::tokio_postgres::Config;
use log::{info, warn};
use regex::Regex;
use rustls::{Certificate, PrivateKey};
use rustls_native_certs::load_native_certs;
use rustls_pemfile::Item::RSAKey;
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::pg::PgError::{
    BadConnectionString, CannotUseClientKey, CantLoadRoots, CantOpenCert, CantParseCert,
    InvalidPrivateKey, UnknownSslMode,
};
use crate::pg::{PgSslCerts, Result};

/// A temporary workaround for <https://github.com/sfackler/rust-postgres/pull/988>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslModeOverride {
    Unmodified(SslMode),
    VerifyCa,
    VerifyFull,
}

/// Special treatment for sslmode=verify-ca & sslmode=verify-full - if found, replace them with sslmode=require
pub fn parse_conn_str(conn_str: &str) -> Result<(Config, SslModeOverride)> {
    let mut mode = SslModeOverride::Unmodified(SslMode::Disable);

    let exp = r"(?P<before>(^|\?|&| )sslmode=)(?P<mode>verify-(ca|full))(?P<after>$|&| )";
    let re = Regex::new(exp).unwrap();
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
    let pg_cfg = pg_cfg.map_err(|e| BadConnectionString(e, conn_str.to_string()))?;
    if let SslModeOverride::Unmodified(_) = mode {
        mode = SslModeOverride::Unmodified(pg_cfg.get_ssl_mode());
    }
    Ok((pg_cfg, mode))
}

struct NoCertificateVerification {}

impl rustls::client::ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &Certificate,
        _intermediates: &[Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp: &[u8],
        _now: std::time::SystemTime,
    ) -> std::result::Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

fn read_certs(file: &PathBuf) -> Result<Vec<Certificate>> {
    Ok(rustls_pemfile::certs(&mut cert_reader(file)?)
        .map_err(|e| CantParseCert(e, file.clone()))?
        .into_iter()
        .map(Certificate)
        .collect())
}

fn cert_reader(file: &PathBuf) -> Result<BufReader<File>> {
    Ok(BufReader::new(
        File::open(file).map_err(|e| CantOpenCert(e, file.clone()))?,
    ))
}

pub fn make_connector(
    pg_certs: &PgSslCerts,
    ssl_mode: SslModeOverride,
) -> Result<MakeRustlsConnect> {
    let (verify_ca, _verify_hostname) = match ssl_mode {
        SslModeOverride::Unmodified(mode) => match mode {
            SslMode::Disable | SslMode::Prefer => (false, false),
            SslMode::Require => match pg_certs.ssl_root_cert {
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

    if let Some(file) = &pg_certs.ssl_root_cert {
        for cert in read_certs(file)? {
            roots.add(&cert)?;
        }
        info!("Using {} as a root certificate", file.display());
    }

    if verify_ca || pg_certs.ssl_root_cert.is_some() || pg_certs.ssl_cert.is_some() {
        let certs = load_native_certs().map_err(CantLoadRoots)?;
        for cert in certs {
            roots.add(&Certificate(cert.0))?;
        }
    }

    let builder = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(roots);

    let mut builder = if let (Some(cert), Some(key)) = (&pg_certs.ssl_cert, &pg_certs.ssl_key) {
        match rustls_pemfile::read_one(&mut cert_reader(key)?)
            .map_err(|e| CantParseCert(e, key.clone()))?
        {
            Some(RSAKey(rsa_key)) => builder
                .with_client_auth_cert(read_certs(cert)?, PrivateKey(rsa_key))
                .map_err(|e| CannotUseClientKey(e, cert.clone(), key.clone()))?,
            _ => Err(InvalidPrivateKey(key.clone()))?,
        }
    } else {
        if pg_certs.ssl_key.is_some() || pg_certs.ssl_key.is_some() {
            warn!("SSL client certificate and key files must be set to use client certificate with Postgres. Only one of them was set.");
        }
        builder.with_no_client_auth()
    };

    if !verify_ca {
        builder
            .dangerous()
            .set_certificate_verifier(std::sync::Arc::new(NoCertificateVerification {}));
    }

    let connector = MakeRustlsConnect::new(builder);

    // TODO: ???
    // if !verify_hostname {
    //     connector.set_callback(|cfg, _domain| {
    //         cfg.set_verify_hostname(false);
    //         Ok(())
    //     });
    // }

    Ok(connector)
}

#[cfg(test)]
mod tests {
    use deadpool_postgres::tokio_postgres::config::Host;

    use super::*;

    #[test]
    fn test_parse_conn_str() {
        let (cfg, mode) =
            parse_conn_str("postgresql://user:password@localhost:5432/dbname").unwrap();
        assert_eq!(cfg.get_hosts(), &vec![Host::Tcp("localhost".to_string())]);
        assert_eq!(cfg.get_ports(), &vec![5432]);
        assert_eq!(cfg.get_user(), Some("user"));
        assert_eq!(cfg.get_dbname(), Some("dbname"));
        assert_eq!(cfg.get_password(), Some(b"password".as_ref()));
        assert_eq!(cfg.get_ssl_mode(), SslMode::Prefer);
        assert_eq!(mode, SslModeOverride::Unmodified(SslMode::Prefer));

        let (cfg, mode) =
            parse_conn_str("postgresql://localhost:5432/db?sslmode=verify-ca").unwrap();
        assert_eq!(cfg.get_ssl_mode(), SslMode::Require);
        assert_eq!(mode, SslModeOverride::VerifyCa);

        let conn = "postgresql://localhost:5432?sslmode=verify-full";
        let (cfg, mode) = parse_conn_str(conn).unwrap();
        assert_eq!(cfg.get_ssl_mode(), SslMode::Require);
        assert_eq!(mode, SslModeOverride::VerifyFull);

        let conn = "postgresql://localhost:5432?sslmode=verify-full&connect_timeout=5";
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
