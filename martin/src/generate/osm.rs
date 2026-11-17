//! The OpenStreetMap sources of `martin cp --mode bulk`: PBF files from the config's `osm` section.

use std::path::Path;

use martin_tilegen::osm::{NodeStore, Objects, OsmTable, columns};
use martin_tilegen::plan::TableDef;

use super::layers::{LowerOptions, SourceSettings, lower_source};
use super::postgres::scan_bbox;
use super::{GenerateError, GenerateOptions, GenerateResult, mlt_encoder};
use crate::config::file::Config;
use crate::config::file::osm::OsmSourceConfig;

/// The smallest and largest partition of a file, in bytes.
const PARTITION_BYTES: std::ops::RangeInclusive<u64> = (1 << 20)..=(64 << 20);

/// The requested OSM sources as tables, or `None` if the run asks for no OSM source.
///
/// # Errors
///
/// If it asks for OSM and other sources together, a layer is not supported, or a file cannot be read.
pub fn select_sources(config: &Config, options: &GenerateOptions) -> GenerateResult<Option<Vec<(TableDef, OsmTable)>>> {
    let osm = &config.osm.sources;
    if osm.is_empty() {
        return Ok(None);
    }
    let requested: Vec<&str> = if options.sources.is_empty() {
        if !config.postgres.is_empty() {
            return Err(GenerateError::MixedSources);
        }
        osm.keys().map(String::as_str).collect()
    } else {
        options.sources.iter().map(String::as_str).collect()
    };
    let (ours, others): (Vec<&str>, Vec<&str>) = requested.into_iter().partition(|id| osm.contains_key(*id));
    if ours.is_empty() {
        return Ok(None);
    }
    if !others.is_empty() {
        return Err(GenerateError::MixedSources);
    }
    let temp_dir = options.temp_dirs.first().cloned().unwrap_or_else(std::env::temp_dir);
    let mut tables = Vec::with_capacity(ours.len());
    for id in ours {
        let source = &osm[id];
        let lower_options = LowerOptions {
            zooms: options.zooms.clone(),
            bounds: options.bbox,
            encoder: mlt_encoder(config.convert_to_mlt.clone(), options.copy_format, id)?,
            order_by: None,
        };
        let Some(def) = lower(id, source, &lower_options)? else {
            continue;
        };
        let table = table(source, &def, &temp_dir, Objects::Taken)?;
        tables.push((def, table));
    }
    if tables.is_empty() {
        return Err(GenerateError::NoLayers(*options.zooms.start(), *options.zooms.end()));
    }
    Ok(Some(tables))
}

/// Source `id` as a table of the plan, or `None` if none of its layers has a zoom in `options`.
///
/// # Errors
///
/// If a layer is not supported.
pub fn lower(id: &str, source: &OsmSourceConfig, options: &LowerOptions) -> GenerateResult<Option<TableDef>> {
    let settings = SourceSettings {
        minzoom: source.minzoom,
        maxzoom: source.maxzoom,
        extent: source.extent,
        buffer: source.buffer,
        clip_geom: source.clip_geom,
        layer_id: None,
        layers: Some(&source.layers),
    };
    lower_source(id, &settings, columns(&source.tags), options)
}

/// The file of `source`, lowered to `def`, reading `objects` with its node store in `temp_dir` if it needs one.
///
/// # Errors
///
/// If the file cannot be read.
pub fn table(source: &OsmSourceConfig, def: &TableDef, temp_dir: &Path, objects: Objects) -> GenerateResult<OsmTable> {
    let node_store = NodeStore::for_file(&source.path, temp_dir).map_err(|error| GenerateError::OsmFile {
        path: source.path.clone(),
        error,
    })?;
    Ok(OsmTable {
        path: source.path.clone(),
        node_store,
        bounds: scan_bbox(&def.layers),
        objects,
    })
}

/// Bytes per partition: each file in about 8 partitions per thread, so threads stay busy to the end.
pub fn partition_bytes(tables: &[(TableDef, OsmTable)], threads: usize) -> u64 {
    let bytes: u64 = tables
        .iter()
        .filter_map(|(_, table)| std::fs::metadata(&table.path).ok())
        .map(|meta| meta.len())
        .sum();
    (bytes / (threads.max(1) as u64 * 8)).clamp(*PARTITION_BYTES.start(), *PARTITION_BYTES.end())
}
