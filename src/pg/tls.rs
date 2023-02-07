use std::str::FromStr;

use deadpool_postgres::tokio_postgres::config::SslMode;
use deadpool_postgres::tokio_postgres::Config;
#[cfg(feature = "ssl")]
use log::{info, warn};
#[cfg(feature = "ssl")]
use openssl::ssl::SslFiletype;
#[cfg(feature = "ssl")]
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use regex::Regex;

use crate::pg::utils::PgError::BadConnectionString;
#[cfg(feature = "ssl")]
use crate::pg::utils::PgError::{BadTrustedRootCertError, BuildSslConnectorError};
use crate::pg::utils::Result;
#[cfg(feature = "ssl")]
use crate::pg::PgError::{BadClientCertError, BadClientKeyError, UnknownSslMode};
use crate::pg::PgSslCerts;

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

#[cfg(not(feature = "ssl"))]
pub fn make_connector(
    _certs: &PgSslCerts,
    _ssl_mode: SslModeOverride,
) -> Result<deadpool_postgres::tokio_postgres::NoTls> {
    Ok(deadpool_postgres::tokio_postgres::NoTls)
}

#[cfg(feature = "ssl")]
pub fn make_connector(
    certs: &PgSslCerts,
    ssl_mode: SslModeOverride,
) -> Result<postgres_openssl::MakeTlsConnector> {
    let (verify_ca, verify_hostname) = match ssl_mode {
        SslModeOverride::Unmodified(mode) => match mode {
            SslMode::Disable | SslMode::Prefer => (false, false),
            SslMode::Require => match certs.ssl_root_cert {
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

    let tls = SslMethod::tls_client();
    let mut builder = SslConnector::builder(tls).map_err(BuildSslConnectorError)?;

    if let (Some(cert), Some(key)) = (&certs.ssl_cert, &certs.ssl_key) {
        builder
            .set_certificate_file(cert, SslFiletype::PEM)
            .map_err(|e| BadClientCertError(e, cert.clone()))?;
        builder
            .set_private_key_file(key, SslFiletype::PEM)
            .map_err(|e| BadClientKeyError(e, key.clone()))?;
    } else if certs.ssl_key.is_some() || certs.ssl_key.is_some() {
        warn!("SSL client certificate and key files must be set to use client certificate with Postgres. Only one of them was set.");
    }

    if let Some(file) = &certs.ssl_root_cert {
        builder
            .set_ca_file(file)
            .map_err(|e| BadTrustedRootCertError(e, file.clone()))?;
        info!("Using {} as a root certificate", file.display());
    }

    if !verify_ca {
        builder.set_verify(SslVerifyMode::NONE);
    }

    let mut connector = postgres_openssl::MakeTlsConnector::new(builder.build());

    if !verify_hostname {
        connector.set_callback(|cfg, _domain| {
            cfg.set_verify_hostname(false);
            Ok(())
        });
    }

    Ok(connector)
}

#[cfg(test)]
mod tests {
    use deadpool_postgres::tokio_postgres::config::Host;

    use super::*;

    #[test]
    fn test_parse_conn_str() {
        let (cfg, mode) = parse_conn_str("postgresql://localhost:5432").unwrap();
        assert_eq!(cfg.get_hosts(), &vec![Host::Tcp("localhost".to_string())]);
        assert_eq!(cfg.get_ports(), &vec![5432]);
        assert_eq!(cfg.get_user(), None);
        assert_eq!(cfg.get_dbname(), None);
        assert_eq!(cfg.get_password(), None);
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
