//! The shapefile sources of `martin cp --mode bulk`: `.shp` or `.zip` files from the config's `shapefile` section.

use std::path::Path;

use martin_tilegen::TileGenError;
use martin_tilegen::plan::TableDef;
use martin_tilegen::shapefile::{Records, Shapefile, ShapefileSource, ShapefileTable};

use super::layers::{LowerOptions, SourceSettings, lower_source};
use super::postgres::scan_bbox;
use super::{GenerateOptions, GenerateResult, mlt_encoder};
use crate::config::file::Config;
use crate::config::file::shapefile::ShapefileSourceConfig;

/// The shapefile sources among `ids` as one source with the tables it reads, or `None` if none of them has a layer
/// with a zoom in `options`.
///
/// # Errors
///
/// If a file cannot be read or a layer is not supported.
pub async fn source(
    config: &Config,
    options: &GenerateOptions,
    ids: &[String],
) -> GenerateResult<Option<(ShapefileSource, Vec<TableDef>)>> {
    let sources: Vec<(String, ShapefileSourceConfig)> = ids
        .iter()
        .filter_map(|id| Some((id.clone(), config.shapefile.sources.get(id)?.clone())))
        .collect();
    if sources.is_empty() {
        return Ok(None);
    }
    let (zooms, bounds, format) = (options.zooms.clone(), options.bbox, options.format);
    let convert = config.convert_to_mlt.clone();
    let temp_dir = options.temp_dirs.first().cloned().unwrap_or_else(std::env::temp_dir);
    tokio::task::spawn_blocking(move || {
        let mut tables = Vec::with_capacity(sources.len());
        for (id, source) in &sources {
            let lower_options = LowerOptions {
                zooms: zooms.clone(),
                bounds,
                encoder: mlt_encoder(convert.clone(), format, id)?,
            };
            let file = open(source, &temp_dir)?;
            let Some(def) = lower(id, source, &file, &lower_options)? else {
                continue;
            };
            let bounds = scan_bbox(&def.layers);
            let records = Records::All;
            tables.push((def, ShapefileTable { file, bounds, records }));
        }
        if tables.is_empty() {
            return Ok(None);
        }
        let defs = tables.iter().map(|(def, _)| def.clone()).collect();
        Ok(Some((ShapefileSource::new(tables)?, defs)))
    })
    .await?
}

/// Opens the file of `source`, unpacking a zip file into `temp_dir`.
///
/// # Errors
///
/// If the file cannot be read, or is not in a coordinate system Martin reads.
pub fn open(source: &ShapefileSourceConfig, temp_dir: &Path) -> GenerateResult<Shapefile> {
    Ok(Shapefile::open(&source.path, temp_dir).map_err(TileGenError::from)?)
}

/// Source `id` over `file` as a table of the plan, or `None` if none of its layers has a zoom in `options`.
///
/// # Errors
///
/// If a layer is not supported.
pub fn lower(
    id: &str,
    source: &ShapefileSourceConfig,
    file: &Shapefile,
    options: &LowerOptions,
) -> GenerateResult<Option<TableDef>> {
    let settings = SourceSettings {
        minzoom: source.minzoom,
        maxzoom: source.maxzoom,
        extent: source.extent,
        buffer: source.buffer,
        clip_geom: source.clip_geom,
        layer_id: None,
        layers: Some(&source.layers),
    };
    lower_source(id, &settings, file.columns(), options)
}
