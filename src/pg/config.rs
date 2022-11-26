use crate::config::{report_unrecognized_config, set_option};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::HashMap;
use std::{env, io};
use tilejson::Bounds;

pub const POOL_SIZE_DEFAULT: u32 = 20;

#[derive(clap::Args, Debug)]
#[command(about, version)]
pub struct PgArgs {
    /// Loads trusted root certificates from a file. The file should contain a sequence of PEM-formatted CA certificates.
    #[cfg(feature = "ssl")]
    #[arg(long)]
    pub ca_root_file: Option<String>,
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

pub trait FormatId {
    fn format_id(&self, db_id: &str) -> String;
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TableInfo {
    /// Table schema
    pub schema: String,

    /// Table name
    pub table: String,

    /// Geometry SRID
    pub srid: u32,

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

    #[serde(flatten, skip_serializing)]
    pub unrecognized: HashMap<String, Value>,
}

impl FormatId for TableInfo {
    fn format_id(&self, db_id: &str) -> String {
        format!(
            "{}.{}.{}.{}",
            db_id, self.schema, self.table, self.geometry_column
        )
    }
}

#[derive(Clone, Serialize, Debug, PartialEq, Default)]
pub struct FunctionInfoDbInfo {
    #[serde(skip_serializing)]
    pub query: String,
    #[serde(skip_serializing)]
    pub has_query_params: bool,
    #[serde(skip_serializing)]
    pub signature: String,
    #[serde(flatten)]
    pub info: FunctionInfo,
}

impl FunctionInfoDbInfo {
    pub fn with_info(&self, info: &FunctionInfo) -> Self {
        Self {
            info: info.clone(),
            ..self.clone()
        }
    }
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
    pub fn new(schema: String, function: String) -> Self {
        Self {
            schema,
            function,
            ..Default::default()
        }
    }

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

impl FormatId for FunctionInfo {
    fn format_id(&self, db_id: &str) -> String {
        format!("{}.{}.{}", db_id, self.schema, self.function)
    }
}

pub type TableInfoSources = HashMap<String, TableInfo>;
pub type TableInfoVec = Vec<TableInfo>;
pub type FuncInfoSources = HashMap<String, FunctionInfo>;
pub type FuncInfoDbSources = HashMap<String, FunctionInfoDbInfo>;
pub type FuncInfoDbMapMap = HashMap<String, HashMap<String, FunctionInfoDbInfo>>;
pub type FuncInfoDbVec = Vec<FunctionInfoDbInfo>;

pub type PgConfig = PgConfigRaw<TableInfoSources, FuncInfoSources>;
pub type PgConfigDb = PgConfigRaw<TableInfoSources, FuncInfoDbSources>;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct PgConfigRaw<T, F> {
    pub connection_string: String,
    #[cfg(feature = "ssl")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_root_file: Option<String>,
    #[cfg(feature = "ssl")]
    pub danger_accept_invalid_certs: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_srid: Option<i32>,
    pub pool_size: u32,
    pub discover_functions: bool,
    pub discover_tables: bool,
    pub table_sources: T,
    pub function_sources: F,
}

#[derive(Debug, Default, PartialEq, Deserialize)]
pub struct PgConfigBuilder {
    pub connection_string: Option<String>,
    #[cfg(feature = "ssl")]
    pub ca_root_file: Option<String>,
    #[cfg(feature = "ssl")]
    pub danger_accept_invalid_certs: Option<bool>,
    pub default_srid: Option<i32>,
    pub pool_size: Option<u32>,
    pub table_sources: Option<TableInfoSources>,
    pub function_sources: Option<FuncInfoSources>,
}

impl PgConfigBuilder {
    pub fn merge(&mut self, other: PgConfigBuilder) -> &mut Self {
        set_option(&mut self.connection_string, other.connection_string);
        #[cfg(feature = "ssl")]
        set_option(&mut self.ca_root_file, other.ca_root_file);
        #[cfg(feature = "ssl")]
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
    pub fn finalize(self) -> io::Result<PgConfig> {
        if let Some(ref ts) = self.table_sources {
            for (k, v) in ts {
                report_unrecognized_config(&format!("table_sources.{}.", k), &v.unrecognized);
            }
        }
        if let Some(ref fs) = self.function_sources {
            for (k, v) in fs {
                report_unrecognized_config(&format!("function_sources.{}.", k), &v.unrecognized);
            }
        }
        let connection_string = self.connection_string.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Other,
                "Database connection string is not set",
            )
        })?;
        Ok(PgConfig {
            connection_string,
            #[cfg(feature = "ssl")]
            ca_root_file: self.ca_root_file,
            #[cfg(feature = "ssl")]
            danger_accept_invalid_certs: self.danger_accept_invalid_certs.unwrap_or_default(),
            default_srid: self.default_srid,
            pool_size: self.pool_size.unwrap_or(POOL_SIZE_DEFAULT),
            discover_functions: self.table_sources.is_none() && self.function_sources.is_none(),
            discover_tables: self.table_sources.is_none() && self.function_sources.is_none(),
            table_sources: self.table_sources.unwrap_or_default(),
            function_sources: self.function_sources.unwrap_or_default(),
        })
    }
}

impl From<(PgArgs, Option<String>)> for PgConfigBuilder {
    fn from((args, connection): (PgArgs, Option<String>)) -> Self {
        PgConfigBuilder {
            connection_string: connection.or_else(|| {
                env::var_os("DATABASE_URL").and_then(|connection| connection.into_string().ok())
            }),
            #[cfg(feature = "ssl")]
            ca_root_file: args.ca_root_file.or_else(|| {
                env::var_os("CA_ROOT_FILE").and_then(|connection| connection.into_string().ok())
            }),
            #[cfg(feature = "ssl")]
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

impl PgConfig {
    pub fn to_db(
        self,
        table_sources: TableInfoSources,
        function_sources: FuncInfoDbSources,
    ) -> PgConfigDb {
        PgConfigDb {
            connection_string: self.connection_string,
            #[cfg(feature = "ssl")]
            ca_root_file: self.ca_root_file,
            #[cfg(feature = "ssl")]
            danger_accept_invalid_certs: self.danger_accept_invalid_certs,
            default_srid: self.default_srid,
            pool_size: self.pool_size,
            discover_functions: self.discover_functions,
            discover_tables: self.discover_tables,
            table_sources,
            function_sources,
        }
    }
}
