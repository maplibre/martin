use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;

use tilejson::{TileJSON, TileJSONBuilder};

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
    pub bounds: Option<Vec<f32>>,

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
            id = self.id,
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
            .map_or("".to_string(), |id_column| format!(", '{}'", id_column));

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

        format!("{} {}", bounds_cte, tile_query)
    }
}

impl Source for TableSource {
    fn get_id(&self) -> &str {
        self.id.as_str()
    }

    fn get_tilejson(&self) -> Result<TileJSON, io::Error> {
        let mut tilejson_builder = TileJSONBuilder::new();

        tilejson_builder.scheme("xyz");
        tilejson_builder.name(&self.id);

        if let Some(minzoom) = &self.minzoom {
            tilejson_builder.minzoom(*minzoom);
        };

        if let Some(maxzoom) = &self.maxzoom {
            tilejson_builder.maxzoom(*maxzoom);
        };

        if let Some(bounds) = &self.bounds {
            tilejson_builder.bounds(bounds.to_vec());
        };

        Ok(tilejson_builder.finalize())
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

pub fn get_table_sources(conn: &mut Connection) -> Result<TableSources, io::Error> {
    let mut sources = HashMap::new();

    let rows = conn
        .query(include_str!("scripts/get_table_sources.sql"), &[])
        .map_err(utils::prettify_error("Can't get table sources".to_owned()))?;

    for row in &rows {
        let schema: String = row.get("f_table_schema");
        let table: String = row.get("f_table_name");
        let id = format!("{}.{}", schema, table);

        let geometry_column: String = row.get("f_geometry_column");
        let srid: i32 = row.get("srid");
        let geometry_type: String = row.get("type");

        info!(
            "Found \"{}\" table source with \"{}\" column ({}, SRID={})",
            id, geometry_column, geometry_type, srid
        );

        if srid == 0 {
            warn!("{} has SRID 0, skipping", id);
            continue;
        }

        let bounds_query = utils::get_source_bounds(&id, srid as u32, &geometry_column);

        let bounds: Option<Vec<f32>> = conn
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

        sources.insert(id, Box::new(source));
    }

    if sources.is_empty() {
        info!("No table sources found");
    }

    Ok(sources)
}
