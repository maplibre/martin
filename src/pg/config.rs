use crate::config::set_option;
use crate::pg::function_source::FunctionSources;
use crate::pg::table_source::TableSources;
use serde::{Deserialize, Serialize};
use std::{env, io};

pub const POOL_SIZE_DEFAULT: u32 = 20;

#[derive(clap::Args, Debug)]
#[command(about, version)]
pub struct PostgreSqlArgs {
    /// Loads trusted root certificates from a file. The file should contain a sequence of PEM-formatted CA certificates.
    #[arg(long)]
    pub ca_root_file: Option<String>,
    /// Trust invalid certificates. This introduces significant vulnerabilities, and should only be used as a last resort.
    #[arg(long)]
    pub danger_accept_invalid_certs: bool,
    /// If a spatial table has SRID 0, then this default SRID will be used as a fallback.
    #[arg(short, long)]
    pub default_srid: Option<i32>,
    #[arg(help = format!("Maximum connections pool size [DEFAULT: {}]", POOL_SIZE_DEFAULT), short, long)]
    pub pool_size: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Config {
    pub connection_string: String,
    pub ca_root_file: Option<String>,
    pub danger_accept_invalid_certs: bool,
    pub default_srid: Option<i32>,
    pub pool_size: u32,
    pub use_dynamic_sources: bool,
    pub table_sources: Option<TableSources>,
    pub function_sources: Option<FunctionSources>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigBuilder {
    pub connection_string: Option<String>,
    pub ca_root_file: Option<String>,
    pub danger_accept_invalid_certs: Option<bool>,
    pub default_srid: Option<i32>,
    pub pool_size: Option<u32>,
    pub table_sources: Option<TableSources>,
    pub function_sources: Option<FunctionSources>,
}

impl ConfigBuilder {
    pub fn merge(&mut self, other: ConfigBuilder) -> &mut Self {
        set_option(&mut self.connection_string, other.connection_string);
        set_option(&mut self.ca_root_file, other.ca_root_file);
        set_option(
            &mut self.danger_accept_invalid_certs,
            other.danger_accept_invalid_certs,
        );
        set_option(&mut self.default_srid, other.default_srid);
        set_option(&mut self.pool_size, other.pool_size);
        set_option(&mut self.table_sources, other.table_sources);
        set_option(&mut self.function_sources, other.function_sources);
        self
    }

    /// Apply defaults to the config, and validate if there is a connection string
    pub fn finalize(self) -> io::Result<Config> {
        let connection_string = self.connection_string.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Other,
                "Database connection string is not set",
            )
        })?;
        Ok(Config {
            connection_string,
            ca_root_file: self.ca_root_file,
            danger_accept_invalid_certs: self.danger_accept_invalid_certs.unwrap_or_default(),
            default_srid: self.default_srid,
            pool_size: self.pool_size.unwrap_or(POOL_SIZE_DEFAULT),
            use_dynamic_sources: self.table_sources.is_none() && self.function_sources.is_none(),
            table_sources: self.table_sources,
            function_sources: self.function_sources,
        })
    }
}

impl From<(PostgreSqlArgs, Option<String>)> for ConfigBuilder {
    fn from((args, connection): (PostgreSqlArgs, Option<String>)) -> Self {
        ConfigBuilder {
            connection_string: connection.or_else(|| {
                env::var_os("DATABASE_URL").and_then(|connection| connection.into_string().ok())
            }),
            ca_root_file: args.ca_root_file.or_else(|| {
                env::var_os("CA_ROOT_FILE").and_then(|connection| connection.into_string().ok())
            }),
            danger_accept_invalid_certs: if args.danger_accept_invalid_certs
                || env::var_os("DANGER_ACCEPT_INVALID_CERTS").is_some()
            {
                Some(true)
            } else {
                None
            },
            default_srid: args.default_srid.or_else(|| {
                env::var_os("DEFAULT_SRID").and_then(|srid| {
                    srid.into_string()
                        .ok()
                        .and_then(|srid| srid.parse::<i32>().ok())
                })
            }),
            pool_size: args.pool_size,
            table_sources: None,
            function_sources: None,
        }
    }
}
