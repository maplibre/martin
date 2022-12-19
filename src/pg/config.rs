use crate::config::{report_unrecognized_config, set_option, OneOrMany};
use crate::pg::configurator::PgBuilder;
use crate::pg::pool::Pool;
use crate::pg::utils::create_tilejson;
use crate::pg::utils::PgError::NoConnectionString;
use crate::pg::utils::Result;
use crate::source::IdResolver;
use crate::srv::server::Sources;
use crate::utils::{get_env_str, InfoMap, Schemas};
use futures::future::try_join;
use itertools::Itertools;
use log::warn;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::{BTreeSet, HashMap};
use tilejson::{Bounds, TileJSON};

pub const POOL_SIZE_DEFAULT: u32 = 20;

#[derive(clap::Args, Debug)]
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct TableInfo {
    /// Table schema
    pub schema: String,

    /// Table name
    pub table: String,

    /// Geometry SRID
    pub srid: i32,

    /// Geometry column name
    pub geometry_column: String,

    /// Feature id column name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_column: Option<String>,

    /// An integer specifying the minimum zoom level
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minzoom: Option<u8>,

    /// An integer specifying the maximum zoom level. MUST be >= minzoom
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maxzoom: Option<u8>,

    /// The maximum extent of available map tiles. Bounds MUST define an area
    /// covered by all zoom levels. The bounds are represented in WGS:84
    /// latitude and longitude values, in the order left, bottom, right, top.
    /// Values may be integers or floating point numbers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,

    /// Tile extent in tile coordinate space
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extent: Option<u32>,

    /// Buffer distance in tile coordinate space to optionally clip geometries
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer: Option<u32>,

    /// Boolean to control if geometries should be clipped or encoded as is
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip_geom: Option<bool>,

    /// Geometry type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry_type: Option<String>,

    /// List of columns, that should be encoded as tile properties
    pub properties: HashMap<String, String>,

    /// Mapping of properties to the actual table columns
    #[serde(skip_deserializing, skip_serializing)]
    pub prop_mapping: HashMap<String, String>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: HashMap<String, Value>,
}

impl PgInfo for TableInfo {
    fn format_id(&self) -> String {
        format!("{}.{}.{}", self.schema, self.table, self.geometry_column)
    }

    fn to_tilejson(&self) -> TileJSON {
        create_tilejson(
            self.format_id(),
            self.minzoom,
            self.maxzoom,
            self.bounds,
            None,
        )
    }
}

pub trait PgInfo {
    fn format_id(&self) -> String;
    fn to_tilejson(&self) -> TileJSON;
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct FunctionInfo {
    /// Schema name
    pub schema: String,

    /// Function name
    pub function: String,

    /// An integer specifying the minimum zoom level
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minzoom: Option<u8>,

    /// An integer specifying the maximum zoom level. MUST be >= minzoom
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maxzoom: Option<u8>,

    /// The maximum extent of available map tiles. Bounds MUST define an area
    /// covered by all zoom levels. The bounds are represented in WGS:84
    /// latitude and longitude values, in the order left, bottom, right, top.
    /// Values may be integers or floating point numbers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: HashMap<String, Value>,
}

impl FunctionInfo {
    #[must_use]
    pub fn new(schema: String, function: String) -> Self {
        Self {
            schema,
            function,
            ..Default::default()
        }
    }

    #[must_use]
    pub fn new_extended(
        schema: String,
        function: String,
        minzoom: u8,
        maxzoom: u8,
        bounds: Bounds,
    ) -> Self {
        Self {
            schema,
            function,
            minzoom: Some(minzoom),
            maxzoom: Some(maxzoom),
            bounds: Some(bounds),
            ..Default::default()
        }
    }
}

impl PgInfo for FunctionInfo {
    fn format_id(&self) -> String {
        format!("{}.{}", self.schema, self.function)
    }

    fn to_tilejson(&self) -> TileJSON {
        create_tilejson(
            self.format_id(),
            self.minzoom,
            self.maxzoom,
            self.bounds,
            None,
        )
    }
}

pub type TableInfoSources = InfoMap<TableInfo>;
pub type FuncInfoSources = InfoMap<FunctionInfo>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PgConfig {
    pub connection_string: Option<String>,
    #[cfg(feature = "ssl")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_root_file: Option<std::path::PathBuf>,
    #[cfg(feature = "ssl")]
    #[serde(default, skip_serializing_if = "Clone::clone")]
    pub danger_accept_invalid_certs: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_srid: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_size: Option<u32>,
    #[serde(skip)]
    pub auto_tables: Option<Schemas>,
    #[serde(skip)]
    pub auto_functions: Option<Schemas>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tables: Option<TableInfoSources>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub functions: Option<FuncInfoSources>,
    #[serde(skip)]
    pub run_autodiscovery: bool,
}

impl PgConfig {
    pub fn merge(&mut self, other: Self) -> &mut Self {
        set_option(&mut self.connection_string, other.connection_string);
        #[cfg(feature = "ssl")]
        {
            set_option(&mut self.ca_root_file, other.ca_root_file);
            self.danger_accept_invalid_certs |= other.danger_accept_invalid_certs;
        }
        set_option(&mut self.default_srid, other.default_srid);
        set_option(&mut self.pool_size, other.pool_size);
        set_option(&mut self.auto_tables, other.auto_tables);
        set_option(&mut self.auto_functions, other.auto_functions);
        set_option(&mut self.tables, other.tables);
        set_option(&mut self.functions, other.functions);
        self
    }

    /// Apply defaults to the config, and validate if there is a connection string
    pub fn finalize(self) -> Result<PgConfig> {
        if let Some(ref ts) = self.tables {
            for (k, v) in ts {
                report_unrecognized_config(&format!("tables.{k}."), &v.unrecognized);
            }
        }
        if let Some(ref fs) = self.functions {
            for (k, v) in fs {
                report_unrecognized_config(&format!("functions.{k}."), &v.unrecognized);
            }
        }
        let connection_string = self.connection_string.ok_or(NoConnectionString)?;

        Ok(PgConfig {
            connection_string: Some(connection_string),
            run_autodiscovery: self.tables.is_none() && self.functions.is_none(),
            ..self
        })
    }

    pub async fn resolve(&mut self, id_resolver: IdResolver) -> Result<(Sources, Pool)> {
        let pg = PgBuilder::new(self, id_resolver).await?;
        let ((mut tables, tbl_info), (funcs, func_info)) =
            try_join(pg.instantiate_tables(), pg.instantiate_functions()).await?;

        self.tables = Some(tbl_info);
        self.functions = Some(func_info);
        tables.extend(funcs);
        Ok((tables, pg.get_pool()))
    }

    #[must_use]
    pub fn is_autodetect(&self) -> bool {
        self.run_autodiscovery
    }
}

#[must_use]
pub fn parse_pg_args(args: &PgArgs, cli_strings: &[String]) -> Option<OneOrMany<PgConfig>> {
    let mut strings = cli_strings
        .iter()
        .filter(|s| is_postgresql_string(s))
        .map(|s| Some(s.to_string()))
        .unique()
        .collect::<BTreeSet<_>>();

    if let Some(s) = get_env_str("DATABASE_URL") {
        if is_postgresql_string(&s) {
            strings.insert(Some(s));
        } else {
            warn!("Environment variable DATABASE_URL is not a postgres connection string");
        }
    }

    if strings.is_empty() {
        // If there are no connection strings in the CLI, try to parse env vars into a single connection
        strings.insert(None);
    }

    let builders: Vec<_> = strings
        .into_iter()
        .map(|s| PgConfig {
            connection_string: s,
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

fn is_postgresql_string(s: &str) -> bool {
    s.starts_with("postgresql://") || s.starts_with("postgres://")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::pg::utils::tests::{assert_config, some_str};
    use indoc::indoc;

    #[test]
    #[allow(clippy::too_many_lines)]
    fn parse_config() {
        assert_config(
            indoc! {"
            ---
            postgres:
              connection_string: 'postgresql://postgres@localhost/db'
        "},
            &Config {
                postgres: Some(vec![PgConfig {
                    connection_string: some_str("postgresql://postgres@localhost/db"),
                    run_autodiscovery: true,
                    ..Default::default()
                }]),
                ..Default::default()
            },
        );

        assert_config(
            indoc! {"
            ---
            postgres:
              - connection_string: 'postgres://postgres@localhost:5432/db'
              - connection_string: 'postgresql://postgres@localhost:5433/db'
        "},
            &Config {
                postgres: Some(vec![
                    PgConfig {
                        connection_string: some_str("postgres://postgres@localhost:5432/db"),
                        run_autodiscovery: true,
                        ..Default::default()
                    },
                    PgConfig {
                        connection_string: some_str("postgresql://postgres@localhost:5433/db"),
                        run_autodiscovery: true,
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            },
        );

        assert_config(
            indoc! {"
            ---
            postgres:
              connection_string: 'postgres://postgres@localhost:5432/db'
              default_srid: 4326
              pool_size: 20
            
              tables:
                table_source:
                  schema: public
                  table: table_source
                  srid: 4326
                  geometry_column: geom
                  id_column: ~
                  minzoom: 0
                  maxzoom: 30
                  bounds: [-180.0, -90.0, 180.0, 90.0]
                  extent: 4096
                  buffer: 64
                  clip_geom: true
                  geometry_type: GEOMETRY
                  properties:
                    gid: int4
            
              functions:
                function_zxy_query:
                  schema: public
                  function: function_zxy_query
                  minzoom: 0
                  maxzoom: 30
                  bounds: [-180.0, -90.0, 180.0, 90.0]
        "},
            &Config {
                postgres: Some(vec![PgConfig {
                    connection_string: some_str("postgres://postgres@localhost:5432/db"),
                    default_srid: Some(4326),
                    pool_size: Some(20),
                    tables: Some(HashMap::from([(
                        "table_source".to_string(),
                        TableInfo {
                            schema: "public".to_string(),
                            table: "table_source".to_string(),
                            srid: 4326,
                            geometry_column: "geom".to_string(),
                            minzoom: Some(0),
                            maxzoom: Some(30),
                            bounds: Some([-180, -90, 180, 90].into()),
                            extent: Some(4096),
                            buffer: Some(64),
                            clip_geom: Some(true),
                            geometry_type: some_str("GEOMETRY"),
                            properties: HashMap::from([("gid".to_string(), "int4".to_string())]),
                            ..Default::default()
                        },
                    )])),
                    functions: Some(HashMap::from([(
                        "function_zxy_query".to_string(),
                        FunctionInfo::new_extended(
                            "public".to_string(),
                            "function_zxy_query".to_string(),
                            0,
                            30,
                            Bounds::MAX,
                        ),
                    )])),
                    ..Default::default()
                }]),
                ..Default::default()
            },
        );
    }
}
