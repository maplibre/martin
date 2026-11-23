//! The shapefile sources of `martin cp --mode bulk`: `.shp` or `.zip` files from the config's `shapefile` section.

use std::ops::RangeInclusive;
use std::path::Path;
use std::sync::Arc;

use martin_tilegen::TileGenError;
use martin_tilegen::plan::TableDef;
use martin_tilegen::shapefile::{Records, Shapefile, ShapefileSource, ShapefileTable};
use tracing::warn;

use super::layers::{LowerOptions, OutputLayers, SourceSettings, lower_source};
use super::postgres::scan_bbox;
use super::{GenerateOptions, GenerateResult, mlt_encoder};
use crate::config::file::Config;
use crate::config::file::shapefile::ShapefileSourceConfig;

/// The shapefile sources among `ids` as one source with the tables it reads, or `None` if none of them has a layer
/// with a zoom in `options`. `outputs` holds the output layers of every source of the run.
///
/// # Errors
///
/// If a file cannot be read or a layer is not supported.
pub async fn source(
    config: &Config,
    options: &GenerateOptions,
    ids: &[String],
    outputs: &Arc<OutputLayers>,
) -> GenerateResult<Option<(ShapefileSource, Vec<TableDef>)>> {
    let sources: Vec<(String, ShapefileSourceConfig)> = ids
        .iter()
        .filter_map(|id| Some((id.clone(), config.shapefile.sources.get(id)?.clone())))
        .collect();
    if sources.is_empty() {
        return Ok(None);
    }
    let (zooms, bounds, format, encoding) = (
        options.zooms.clone(),
        options.bbox,
        options.copy_format,
        options.encoding,
    );
    let convert = config.convert_to_mlt.clone();
    let temp_dir = options.temp_dirs.first().cloned().unwrap_or_else(std::env::temp_dir);
    let outputs = Arc::clone(outputs);
    tokio::task::spawn_blocking(move || {
        let mut tables = Vec::with_capacity(sources.len());
        for (id, source) in &sources {
            let lower_options = LowerOptions {
                zooms: zooms.clone(),
                bounds,
                encoder: mlt_encoder(convert.clone(), format, encoding, id)?,
                order_by: None,
            };
            if !has_zoom_in(source, &zooms) {
                warn!(
                    "Skipping source `{id}`: it has no zoom between {} and {}",
                    zooms.start(),
                    zooms.end()
                );
                continue;
            }
            let file = open(source, &temp_dir)?;
            let Some(def) = lower_with(id, source, &file, &lower_options, &outputs)? else {
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

/// Whether `source` has a zoom in `zooms`, so its file needs opening and, for a zip file, unpacking.
fn has_zoom_in(source: &ShapefileSourceConfig, zooms: &RangeInclusive<u8>) -> bool {
    source.minzoom.unwrap_or(0).max(*zooms.start()) <= source.maxzoom.unwrap_or(u8::MAX).min(*zooms.end())
}

/// Opens the file of `source`, unpacking a zip file into `temp_dir`.
///
/// # Errors
///
/// If the file cannot be read, or is not in a coordinate system Martin reads.
pub fn open(source: &ShapefileSourceConfig, temp_dir: &Path) -> GenerateResult<Shapefile> {
    Ok(Shapefile::open(&source.path, temp_dir).map_err(TileGenError::from)?)
}

/// Source `id` over `file` as the only table of a plan, or `None` if none of its layers has a zoom in `options`.
///
/// # Errors
///
/// If a layer is not supported, or layers writing into one output layer disagree on a setting it has once.
pub fn lower(
    id: &str,
    source: &ShapefileSourceConfig,
    file: &Shapefile,
    options: &LowerOptions,
) -> GenerateResult<Option<TableDef>> {
    let outputs = OutputLayers::new([(id, settings(source))], &options.zooms)?;
    lower_with(id, source, file, options, &outputs)
}

/// Source `id` over `file` as a table of a plan with the output layers `outputs`, or `None` if none of its layers
/// has a zoom in `options`.
///
/// # Errors
///
/// If a layer is not supported.
fn lower_with(
    id: &str,
    source: &ShapefileSourceConfig,
    file: &Shapefile,
    options: &LowerOptions,
    outputs: &OutputLayers,
) -> GenerateResult<Option<TableDef>> {
    lower_source(id, &settings(source), file.columns(), options, outputs)
}

/// The settings of `source` that its layers take their defaults from.
#[must_use]
pub fn settings(source: &ShapefileSourceConfig) -> SourceSettings<'_> {
    SourceSettings {
        minzoom: source.minzoom,
        maxzoom: source.maxzoom,
        extent: source.extent,
        buffer: source.buffer,
        clip_geom: source.clip_geom,
        layer_id: None,
        layers: Some(&source.layers),
    }
}
