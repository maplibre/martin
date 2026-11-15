//! The OpenStreetMap sources of `martin cp --mode bulk`: PBF files from the config's `osm` section.

use std::path::Path;

use martin_tilegen::osm::{NodeStore, Objects, OsmSource, OsmTable, columns};
use martin_tilegen::plan::TableDef;

use super::layers::{LowerOptions, SourceSettings, lower_source};
use super::postgres::scan_bbox;
use super::{GenerateError, GenerateOptions, GenerateResult, mlt_encoder};
use crate::config::file::Config;
use crate::config::file::osm::OsmSourceConfig;

/// The smallest and largest partition of a file, in bytes.
const PARTITION_BYTES: std::ops::RangeInclusive<u64> = (1 << 20)..=(64 << 20);

/// The OSM sources among `ids` as one source with the tables it reads, or `None` if none of them has a layer with a
/// zoom in `options`.
///
/// # Errors
///
/// If a layer is not supported, or a file cannot be read.
pub async fn source(
    config: &Config,
    options: &GenerateOptions,
    ids: &[String],
) -> GenerateResult<Option<(OsmSource, Vec<TableDef>)>> {
    let temp_dir = options.temp_dirs.first().cloned().unwrap_or_else(std::env::temp_dir);
    let mut tables = Vec::new();
    for id in ids {
        let Some(source) = config.osm.sources.get(id) else {
            continue;
        };
        let lower_options = LowerOptions {
            zooms: options.zooms.clone(),
            bounds: options.bbox,
            encoder: mlt_encoder(config.convert_to_mlt.clone(), options.copy_format, options.encoding, id)?,
            order_by: None,
        };
        let Some(def) = lower(id, source, &lower_options)? else {
            continue;
        };
        let table = table(source, &def, &temp_dir, Objects::Taken)?;
        tables.push((def, table));
    }
    if tables.is_empty() {
        return Ok(None);
    }
    let defs = tables.iter().map(|(def, _)| def.clone()).collect();
    let partition_bytes = partition_bytes(&tables, options.threads);
    let source = tokio::task::spawn_blocking(move || OsmSource::new(tables, partition_bytes)).await??;
    Ok(Some((source, defs)))
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
