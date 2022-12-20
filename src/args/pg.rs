use crate::one_or_many::OneOrMany;
use crate::pg::config;
use crate::pg::config::PgConfig;
use crate::pg::pool::POOL_SIZE_DEFAULT;
use crate::utils::get_env_str;
use itertools::Itertools;
use log::warn;
use std::collections::BTreeSet;

#[derive(clap::Args, Debug, Default)]
#[command(about, version)]
pub struct PgArgs {
    /// Loads trusted root certificates from a file. The file should contain a sequence of PEM-formatted CA certificates.
    #[cfg(feature = "ssl")]
    #[arg(long)]
    pub ca_root_file: Option<std::path::PathBuf>,
    /// Trust invalid certificates. This introduces significant vulnerabilities, and should only be used as a last resort.
    #[cfg(feature = "ssl")]
    #[arg(long)]
    pub danger_accept_invalid_certs: bool,
    /// If a spatial table has SRID 0, then this default SRID will be used as a fallback.
    #[arg(short, long)]
    pub default_srid: Option<i32>,
    #[arg(help = format!("Maximum connections pool size [DEFAULT: {}]", POOL_SIZE_DEFAULT), short, long)]
    pub pool_size: Option<u32>,
}

#[must_use]
pub fn parse_pg_args(args: &PgArgs, cli_strings: &[String]) -> Option<OneOrMany<PgConfig>> {
    let mut strings = cli_strings
        .iter()
        .filter(|s| config::is_postgresql_string(s))
        .map(std::string::ToString::to_string)
        .unique()
        .collect::<BTreeSet<_>>();

    if let Some(s) = get_env_str("DATABASE_URL") {
        if config::is_postgresql_string(&s) {
            strings.insert(s);
        } else {
            warn!("Environment variable DATABASE_URL is not a postgres connection string");
        }
    }

    let builders: Vec<_> = strings
        .into_iter()
        .map(|s| PgConfig {
            connection_string: Some(s),
            #[cfg(feature = "ssl")]
            ca_root_file: args
                .ca_root_file
                .clone()
                .or_else(|| std::env::var_os("CA_ROOT_FILE").map(std::path::PathBuf::from)),
            #[cfg(feature = "ssl")]
            danger_accept_invalid_certs: args.danger_accept_invalid_certs
                || get_env_str("DANGER_ACCEPT_INVALID_CERTS").is_some(),
            default_srid: args.default_srid.or_else(|| {
                get_env_str("DEFAULT_SRID").and_then(|srid| match srid.parse::<i32>() {
                    Ok(v) => Some(v),
                    Err(v) => {
                        warn!("Env var DEFAULT_SRID is not a valid integer {srid}: {v}");
                        None
                    }
                })
            }),
            pool_size: args.pool_size,
            ..Default::default()
        })
        .collect();

    match builders.len() {
        0 => None,
        1 => Some(OneOrMany::One(builders.into_iter().next().unwrap())),
        _ => Some(OneOrMany::Many(builders)),
    }
}
