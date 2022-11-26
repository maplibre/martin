use crate::pg::config::{FormatId, TableInfo, TableInfoSources, TableInfoVec};
use crate::pg::db::get_connection;
use crate::pg::db::Pool;
use crate::pg::utils::{
    creat_tilejson, get_bounds_cte, get_source_bounds, get_srid_bounds, is_valid_zoom,
    json_to_hashmap, polygon_to_bbox, prettify_error, tile_bbox,
};
use crate::source::{Source, Tile, UrlQuery, Xyz};
use async_trait::async_trait;
use log::{info, warn};
use martin_tile_utils::DataFormat;
use std::collections::{HashMap, HashSet};
use std::io;
use tilejson::{TileJSON, VectorLayer};

#[derive(Clone, Debug)]
pub struct TableSource {
    pub id: String,
    pub info: TableInfo,
    pool: Pool,
    tilejson: TileJSON,
}

pub type TableSources = HashMap<String, Box<TableSource>>;

impl TableSource {
    pub fn new(id: String, info: TableInfo, pool: Pool) -> Self {
        let mut layer = VectorLayer::new(id.clone(), info.properties.clone());
        layer.minzoom = info.minzoom;
        layer.maxzoom = info.maxzoom;
        Self {
            tilejson: creat_tilejson(
                format!("{}.{}.{}", info.schema, info.table, info.geometry_column),
                info.minzoom,
                info.maxzoom,
                info.bounds,
                Some(vec![layer]),
            ),
            id,
            info,
            pool,
        }
    }

    pub fn get_geom_query(&self, xyz: &Xyz) -> String {
        let mercator_bounds = tile_bbox(xyz);

        let info = &self.info;
        let properties = if info.properties.is_empty() {
            String::new()
        } else {
            let properties = info
                .properties
                .keys()
                .map(|column| format!(r#""{column}""#))
                .collect::<Vec<String>>()
                .join(",");

            format!(", {properties}")
        };

        format!(
            include_str!("scripts/get_geom.sql"),
            schema = info.schema,
            table = info.table,
            srid = info.srid,
            geometry_column = info.geometry_column,
            mercator_bounds = mercator_bounds,
            extent = info.extent.unwrap_or(DEFAULT_EXTENT),
            buffer = info.buffer.unwrap_or(DEFAULT_BUFFER),
            clip_geom = info.clip_geom.unwrap_or(DEFAULT_CLIP_GEOM),
            properties = properties
        )
    }

    pub fn get_tile_query(&self, xyz: &Xyz) -> String {
        let geom_query = self.get_geom_query(xyz);

        let id_column = self
            .info
            .id_column
            .clone()
            .map_or(String::new(), |id_column| format!(", '{id_column}'"));

        format!(
            include_str!("scripts/get_tile.sql"),
            id = self.id,
            id_column = id_column,
            geom_query = geom_query,
            extent = self.info.extent.unwrap_or(DEFAULT_EXTENT),
        )
    }

    pub fn build_tile_query(&self, xyz: &Xyz) -> String {
        let srid_bounds = get_srid_bounds(self.info.srid, xyz);
        let bounds_cte = get_bounds_cte(&srid_bounds);
        let tile_query = self.get_tile_query(xyz);

        format!("{bounds_cte} {tile_query}")
    }
}

#[async_trait]
impl Source for TableSource {
    fn get_tilejson(&self) -> TileJSON {
        self.tilejson.clone()
    }

    fn get_format(&self) -> DataFormat {
        DataFormat::Mvt
    }

    fn clone_source(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }

    fn is_valid_zoom(&self, zoom: i32) -> bool {
        is_valid_zoom(zoom, self.info.minzoom, self.info.maxzoom)
    }

    async fn get_tile(&self, xyz: &Xyz, _query: &Option<UrlQuery>) -> Result<Tile, io::Error> {
        let tile_query = self.build_tile_query(xyz);
        let conn = get_connection(&self.pool).await?;
        let tile: Tile = conn
            .query_one(tile_query.as_str(), &[])
            .await
            .map(|row| row.get("st_asmvt"))
            .map_err(|error| {
                prettify_error!(
                    error,
                    r#"Can't get "{}" tile at /{}/{}/{}"#,
                    self.id,
                    xyz.z,
                    xyz.x,
                    xyz.z
                )
            })?;

        Ok(tile)
    }
}

static DEFAULT_EXTENT: u32 = 4096;
static DEFAULT_BUFFER: u32 = 64;
static DEFAULT_CLIP_GEOM: bool = true;

pub async fn get_table_sources(
    pool: &Pool,
    explicit_tables: &TableInfoSources,
    default_srid: Option<i32>,
) -> Result<TableInfoVec, io::Error> {
    let skip_tables: HashSet<String> = explicit_tables.values().map(|v| v.format_id("")).collect();
    let conn = get_connection(pool).await?;
    let rows = conn
        .query(include_str!("scripts/get_table_sources.sql"), &[])
        .await
        .map_err(|e| prettify_error!(e, "Can't get table sources"))?;

    let mut result = TableInfoVec::default();
    for row in &rows {
        let schema: String = row.get("f_table_schema");
        let table: String = row.get("f_table_name");
        let geometry_column: String = row.get("f_geometry_column");
        let srid: i32 = row.get("srid");

        let mut info = TableInfo {
            schema,
            table,
            id_column: None,
            geometry_column,
            bounds: None,
            minzoom: None,
            maxzoom: None,
            srid: srid as u32,
            extent: Some(DEFAULT_EXTENT),
            buffer: Some(DEFAULT_BUFFER),
            clip_geom: Some(DEFAULT_CLIP_GEOM),
            geometry_type: row.get("type"),
            properties: json_to_hashmap(&row.get("properties")),
            unrecognized: HashMap::new(),
        };

        if skip_tables.contains(&info.format_id("")) {
            continue;
        }

        // FIXME: in theory, schema or table can be arbitrary, and thus may cause SQL injection
        let table_id = format!("{}.{}", info.schema, info.table);

        if srid == 0 {
            if let Some(default_srid) = default_srid {
                warn!(r#""{table_id}" has SRID 0, using the provided default SRID {default_srid}"#);
                info.srid = default_srid as u32;
            } else {
                warn!(
                    r#""{table_id}" has SRID 0, skipping. To use this table source, you must specify the SRID using the config file or provide the default SRID"#
                );
                continue;
            }
        }

        let bounds_query = get_source_bounds(&table_id, srid as u32, &info.geometry_column);
        info.bounds = conn
            .query_one(bounds_query.as_str(), &[])
            .await
            .map(|row| row.get("bounds"))
            .ok()
            .flatten()
            .and_then(|v| polygon_to_bbox(&v));

        result.push(info);
    }
    result.sort_by_key(|v| v.table.clone());

    if result.is_empty() {
        info!("No table sources found");
    }

    Ok(result)
}
