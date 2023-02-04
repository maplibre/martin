use crate::pg::{utils, PgConfig};
use bb8::ErrorSink;
use bb8_postgres::PostgresConnectionManager;
use log::error;

#[cfg(feature = "ssl")]
use crate::pg::utils::PgError::{BadTrustedRootCertError, BuildSslConnectorError};
#[cfg(feature = "ssl")]
use log::info;
#[cfg(feature = "ssl")]
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};

#[cfg(feature = "ssl")]
pub type ConnectionManager = PostgresConnectionManager<postgres_openssl::MakeTlsConnector>;
#[cfg(not(feature = "ssl"))]
pub type ConnectionManager = PostgresConnectionManager<postgres::NoTls>;

#[derive(Debug, Clone, Copy)]
pub struct PgErrorSink;

type PgConnError = <ConnectionManager as bb8::ManageConnection>::Error;

impl ErrorSink<PgConnError> for PgErrorSink {
    fn sink(&self, e: PgConnError) {
        error!("{e}");
    }

    fn boxed_clone(&self) -> Box<dyn ErrorSink<PgConnError>> {
        Box::new(*self)
    }
}

#[cfg(not(feature = "ssl"))]
pub fn make_connector(_config: &PgConfig) -> utils::Result<postgres::NoTls> {
    Ok(postgres::NoTls)
}

#[cfg(feature = "ssl")]
pub fn make_connector(config: &PgConfig) -> utils::Result<postgres_openssl::MakeTlsConnector> {
    let tls = SslMethod::tls();
    let mut builder = SslConnector::builder(tls).map_err(BuildSslConnectorError)?;

    if let Some(file) = &config.ca_root_file {
        builder
            .set_ca_file(file)
            .map_err(|e| BadTrustedRootCertError(e, file.clone()))?;
        info!("Using {} as trusted root certificate", file.display());
    } else {
        // TODO: Once https://github.com/sfackler/rust-postgres/pull/988 is merged,
        // we can only set this to None if ssl mode is Required, but not verify*
        builder.set_verify(SslVerifyMode::NONE);
    }

    let mut connector = postgres_openssl::MakeTlsConnector::new(builder.build());

    // TODO: Once https://github.com/sfackler/rust-postgres/pull/988 is merged,
    // we can only only do this if ssl mode is Required, but not verifyFull
    connector.set_callback(|cfg, _domain| {
        cfg.set_verify_hostname(false);
        Ok(())
    });

    Ok(connector)
}
