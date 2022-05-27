use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io;

use tilejson::{tilejson, Bounds, TileJSON};

use crate::db::Connection;
use crate::source::{Query, Source, Tile, Xyz};
use crate::utils;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableSource {
    // Table source id
    pub id: String,

    // Table schema
    pub schema: String,

    // Table name
    pub table: String,

    // Geometry SRID
    pub srid: u32,

    // Geometry column name
    pub geometry_column: String,

    // Feature id column name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_column: Option<String>,

    // An integer specifying the minimum zoom level
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minzoom: Option<u8>,

    // An integer specifying the maximum zoom level. MUST be >= minzoom
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maxzoom: Option<u8>,

    // The maximum extent of available map tiles. Bounds MUST define an area
    // covered by all zoom levels. The bounds are represented in WGS:84
    // latitude and longitude values, in the order left, bottom, right, top.
    // Values may be integers or floating point numbers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,

    // Tile extent in tile coordinate space
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extent: Option<u32>,

    // Buffer distance in tile coordinate space to optionally clip geometries
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer: Option<u32>,

    // Boolean to control if geometries should be clipped or encoded as is
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip_geom: Option<bool>,

    // Geometry type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry_type: Option<String>,

    // List of columns, that should be encoded as tile properties
    pub properties: HashMap<String, String>,
}

pub type TableSources = HashMap<String, Box<TableSource>>;

impl TableSource {
    pub fn get_geom_query(&self, xyz: &Xyz) -> String {
        let mercator_bounds = utils::tilebbox(xyz);

        let properties = if self.properties.is_empty() {
            String::new()
        } else {
            let properties = self
                .properties
                .keys()
                .map(|column| format!("\"{0}\"", column))
                .collect::<Vec<String>>()
                .join(",");

            format!(", {0}", properties)
        };

        format!(
            include_str!("scripts/get_geom.sql"),
            schema = self.schema,
            table = self.table,
            srid = self.srid,
            geometry_column = self.geometry_column,
            mercator_bounds = mercator_bounds,
            extent = self.extent.unwrap_or(DEFAULT_EXTENT),
            buffer = self.buffer.unwrap_or(DEFAULT_BUFFER),
            clip_geom = self.clip_geom.unwrap_or(DEFAULT_CLIP_GEOM),
            properties = properties
        )
    }

    pub fn get_tile_query(&self, xyz: &Xyz) -> String {
        let geom_query = self.get_geom_query(xyz);

        let id_column = self
            .id_column
            .clone()
            .map_or("".to_string(), |id_column| format!(", '{id_column}'"));

        format!(
            include_str!("scripts/get_tile.sql"),
            id = self.id,
            id_column = id_column,
            geom_query = geom_query,
            extent = self.extent.unwrap_or(DEFAULT_EXTENT),
        )
    }

    pub fn build_tile_query(&self, xyz: &Xyz) -> String {
        let srid_bounds = utils::get_srid_bounds(self.srid, xyz);
        let bounds_cte = utils::get_bounds_cte(srid_bounds);
        let tile_query = self.get_tile_query(xyz);

        format!("{bounds_cte} {tile_query}")
    }
}

impl Source for TableSource {
    fn get_id(&self) -> &str {
        self.id.as_str()
    }

    fn get_tilejson(&self) -> Result<TileJSON, io::Error> {
        let mut tilejson = tilejson! {
            tilejson: "2.2.0".to_string(),
            tiles: vec![],  // tile source is required, but not yet known
            name: self.id.to_string(),
        };

        if let Some(minzoom) = &self.minzoom {
            tilejson.minzoom = Some(*minzoom);
        };

        if let Some(maxzoom) = &self.maxzoom {
            tilejson.maxzoom = Some(*maxzoom);
        };

        if let Some(bounds) = &self.bounds {
            tilejson.bounds = Some(*bounds);
        };

        // TODO: consider removing - this is not needed per TileJSON spec
        tilejson.set_missing_defaults();
        Ok(tilejson)
    }

    fn get_tile(
        &self,
        conn: &mut Connection,
        xyz: &Xyz,
        _query: &Option<Query>,
    ) -> Result<Tile, io::Error> {
        let tile_query = self.build_tile_query(xyz);

        let tile: Tile = conn
            .query_one(tile_query.as_str(), &[])
            .map(|row| row.get("st_asmvt"))
            .map_err(utils::prettify_error(format!(
                "Can't get \"{}\" tile at /{}/{}/{}",
                self.id, &xyz.z, &xyz.x, &xyz.z
            )))?;

        Ok(tile)
    }
}

static DEFAULT_EXTENT: u32 = 4096;
static DEFAULT_BUFFER: u32 = 64;
static DEFAULT_CLIP_GEOM: bool = true;

pub fn get_table_sources(
    conn: &mut Connection,
    default_srid: &Option<i32>,
) -> Result<TableSources, io::Error> {
    let mut sources = HashMap::new();
    let mut duplicate_source_ids = HashSet::new();

    let rows = conn
        .query(include_str!("scripts/get_table_sources.sql"), &[])
        .map_err(utils::prettify_error("Can't get table sources".to_owned()))?;

    for row in &rows {
        let schema: String = row.get("f_table_schema");
        let table: String = row.get("f_table_name");
        let geometry_column: String = row.get("f_geometry_column");

        let mut srid: i32 = row.get("srid");
        let geometry_type: String = row.get("type");

        let id = format!("{schema}.{table}");
        let explicit_id = format!("{schema}.{table}.{geometry_column}");

        if sources.contains_key(&id) {
            duplicate_source_ids.insert(id.to_owned());
        } else {
            info!(
                "Found \"{id}\" table source with \"{geometry_column}\" column ({geometry_type}, SRID={srid})"

            );
        }

        if srid == 0 {
            match default_srid {
                Some(default_srid) => {
                    warn!("\"{id}\" has SRID 0, using the provided default SRID {default_srid}");
                    srid = *default_srid;
                }
                None => {
                    warn!("\"{id}\" has SRID 0, skipping. To use this table source, you must specify the SRID using the config file or provide the default SRID");
                    continue;
                }
            }
        }

        let bounds_query = utils::get_source_bounds(&id, srid as u32, &geometry_column);

        let bounds: Option<Bounds> = conn
            .query_one(bounds_query.as_str(), &[])
            .map(|row| row.get("bounds"))
            .ok()
            .flatten()
            .and_then(utils::polygon_to_bbox);

        let properties = utils::json_to_hashmap(&row.get("properties"));

        let source = TableSource {
            id: id.to_owned(),
            schema,
            table,
            id_column: None,
            geometry_column,
            bounds,
            minzoom: None,
            maxzoom: None,
            srid: srid as u32,
            extent: Some(DEFAULT_EXTENT),
            buffer: Some(DEFAULT_BUFFER),
            clip_geom: Some(DEFAULT_CLIP_GEOM),
            geometry_type: row.get("type"),
            properties,
        };

        let mut explicit_source = source.clone();
        explicit_source.id = explicit_id.to_owned();

        sources.entry(id).or_insert_with(|| Box::new(source));
        sources.insert(explicit_id, Box::new(explicit_source));
    }

    if sources.is_empty() {
        info!("No table sources found");
    }

    if !duplicate_source_ids.is_empty() {
        let sources = duplicate_source_ids
            .into_iter()
            .collect::<Vec<String>>()
            .join(", ");

        warn!("These table sources have multiple geometry columns: {sources}");

        warn!(
            "You can specify the geometry column in the table source name to access particular geometry in vector tile, eg. \"schema_name.table_name.geometry_column\"",
        );
    }

    Ok(sources)
}
