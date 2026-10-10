//! Runs a whole generation: render every feature into sorted runs, then merge, encode and write tiles.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::{self, ScopedJoinHandle};

use geo_types::Coord;

use martin_tile_utils::Encoding;
use mlt_core::PropKind;
use tilejson::{TileJSON, VectorLayer};

use crate::pipeline::{self, Submit};
use crate::plan::{AttributesDef, IdDef, Plan, PlannedLayer};
use crate::props::{KeyId, KeyInterner, KeyNames};
use crate::record::EncodedProps;
use crate::source::{Crs, FeatureBatch, FeatureSource, Geometry};
use crate::{
    DedupIndex, EncodeSettings, Feature, FeatureGeom, LayerInfo, LayerStats, Renderer, Seq,
    SortBuffer, SortConfig, Sorter, TileEncoder, TileFormat, TileGenError, TileGenResult,
    TileGrouper, TileOrder, TileRecords, TileSink, project,
};

#[derive(Clone, Debug)]
pub struct GenerateConfig {
    pub threads: usize,
    /// `buffer_bytes` is per render worker.
    pub sort: SortConfig,
    pub format: TileFormat,
    pub encoding: Encoding,
    /// Tiles per encode batch: large enough to amortize hand-offs, small enough to keep encoders busy.
    pub batch_tiles: usize,
    /// Encode batches in flight.
    pub window: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub features: u64,
    pub tiles: u64,
    /// Feature-zooms the slicer could not handle and skipped.
    pub slice_errors: u64,
    pub layers: Vec<LayerStats>,
}

/// Live counters another thread can report while a generation runs.
#[derive(Debug, Default)]
pub struct Progress {
    pub features: AtomicU64,
    pub tiles: AtomicU64,
    /// Set when rendering is done and tiles are being merged, encoded and written.
    pub writing: AtomicBool,
}

/// Generates every tile of `plan` from `source` into `sink`. `metadata` is completed with the zooms, `format` and
/// `vector_layers` the run produced, then stored once all tiles are written.
pub fn generate<S: FeatureSource, K: TileSink>(
    source: &S,
    plan: &Plan,
    mut sink: K,
    config: &GenerateConfig,
    mut metadata: TileJSON,
    progress: &Progress,
) -> TileGenResult<Summary> {
    let order = sink.tile_order();
    let keys = Keys {
        tables: plan
            .tables
            .iter()
            .map(|table| KeyInterner::new(&table.columns))
            .collect(),
        layers: plan
            .layers
            .iter()
            .map(|layer| KeyInterner::new(&layer.keys))
            .collect(),
    };

    let sorter = Sorter::new(config.sort.clone())?;
    let totals = render_all(
        source,
        &sorter,
        plan,
        &keys,
        order,
        config.threads,
        progress,
    )?;
    progress.writing.store(true, Ordering::Relaxed);

    let names: Vec<KeyNames> = keys.layers.into_iter().map(KeyInterner::freeze).collect();
    let infos: Vec<_> = plan.layers.iter().map(|layer| layer.info.clone()).collect();
    let settings = EncodeSettings {
        layers: &infos,
        keys: &names,
        format: config.format,
        encoding: config.encoding,
        order,
    };
    let dedup = DedupIndex::default();
    let mut grouper = TileGrouper::new(sorter.merge()?);
    let batch_tiles = config.batch_tiles.max(1);
    let ((sink, tiles), encoders) = pipeline::run(
        config.threads,
        config.window.max(1),
        |submit: &mut Submit<Vec<TileRecords>>| loop {
            let mut batch = Vec::with_capacity(batch_tiles);
            while batch.len() < batch_tiles {
                let mut tile = TileRecords::default();
                if !grouper.next_tile(&mut tile)? {
                    break;
                }
                batch.push(tile);
            }
            let done = batch.len() < batch_tiles;
            if !batch.is_empty() {
                submit.send(batch)?;
            }
            if done {
                return Ok(());
            }
        },
        TileEncoder::default,
        |encoder, batch| encoder.encode_batch(&settings, &dedup, batch),
        move |batches| {
            let mut tiles = 0u64;
            sink.write_all(&mut batches.inspect(|batch| {
                if let Ok(batch) = batch {
                    tiles += batch.len() as u64;
                    progress
                        .tiles
                        .fetch_add(batch.len() as u64, Ordering::Relaxed);
                }
            }))?;
            Ok((sink, tiles))
        },
    )?;

    let mut layers = vec![LayerStats::default(); infos.len()];
    for encoder in &encoders {
        for (total, stats) in layers.iter_mut().zip(&encoder.stats) {
            total.merge(stats);
        }
    }
    complete_metadata(&mut metadata, &infos, &layers, config.format);
    sink.finish(&metadata)?;
    Ok(Summary {
        features: totals.features,
        tiles,
        slice_errors: totals.slice_errors,
        layers,
    })
}

/// Property keys as sources intern them, per table, and as tiles hold them, per output layer.
struct Keys {
    tables: Vec<KeyInterner>,
    layers: Vec<KeyInterner>,
}

#[derive(Default)]
struct RenderTotals {
    features: u64,
    slice_errors: u64,
}

fn render_all<S: FeatureSource>(
    source: &S,
    sorter: &Sorter,
    plan: &Plan,
    keys: &Keys,
    order: TileOrder,
    threads: usize,
    progress: &Progress,
) -> TileGenResult<RenderTotals> {
    let threads = threads.max(1);
    let partitions = source.partitions();
    let next_partition = AtomicU32::new(0);
    let failed = AtomicBool::new(false);
    let (tx, rx) = flume::bounded::<FeatureBatch>(threads * 2);
    let fail = |err| {
        failed.store(true, Ordering::Relaxed);
        err
    };

    let (next_partition, failed, fail) = (&next_partition, &failed, &fail);
    thread::scope(|scope| {
        let readers: Vec<_> = source
            .open_readers(threads.min(partitions as usize))?
            .into_iter()
            .map(|mut reader| {
                let tx = tx.clone();
                scope.spawn(move || -> TileGenResult<()> {
                    let tx = tx;
                    loop {
                        let partition = next_partition.fetch_add(1, Ordering::Relaxed);
                        if partition >= partitions || failed.load(Ordering::Relaxed) {
                            return Ok(());
                        }
                        reader
                            .read(partition, &keys.tables, &mut |batch| {
                                tx.send(batch)
                                    .map_err(|_closed| TileGenError::WriterStopped)
                            })
                            .map_err(fail)?;
                    }
                })
            })
            .collect();
        drop(tx);
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                let rx = rx.clone();
                scope.spawn(|| -> TileGenResult<RenderTotals> {
                    let mut buffer = sorter.buffer();
                    let mut worker = Worker {
                        renderer: Renderer::default(),
                        props: EncodedProps::default(),
                        dynamic: vec![Vec::new(); plan.layers.len()],
                    };
                    let mut count = 0;
                    for mut batch in rx {
                        if failed.load(Ordering::Relaxed) {
                            break;
                        }
                        render_batch(&mut batch, &mut worker, plan, keys, order, &mut buffer)
                            .map_err(fail)?;
                        count += batch.features.len() as u64;
                        progress
                            .features
                            .fetch_add(batch.features.len() as u64, Ordering::Relaxed);
                    }
                    buffer.finish().map_err(fail)?;
                    Ok(RenderTotals {
                        features: count,
                        slice_errors: worker.renderer.slice_errors,
                    })
                })
            })
            .collect();
        drop(rx);

        let mut errors = Vec::new();
        for reader in readers {
            if let Err(err) = join(reader) {
                errors.push(err);
            }
        }
        let mut totals = RenderTotals::default();
        for worker in workers {
            match join(worker) {
                Ok(worker) => {
                    totals.features += worker.features;
                    totals.slice_errors += worker.slice_errors;
                }
                Err(err) => errors.push(err),
            }
        }
        match errors
            .into_iter()
            .min_by_key(|err| matches!(err, TileGenError::WriterStopped))
        {
            Some(err) => Err(err),
            None => Ok(totals),
        }
    })
}

fn join<T>(handle: ScopedJoinHandle<'_, T>) -> T {
    handle
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

struct Worker {
    renderer: Renderer,
    props: EncodedProps,
    /// Output key of each table key interned after the declared columns, by layer; filled lazily.
    dynamic: Vec<Vec<DynamicKey>>,
}

#[derive(Clone, Copy)]
enum DynamicKey {
    Unresolved,
    Resolved(Option<KeyId>),
}

impl Worker {
    fn output_key(
        &mut self,
        index: usize,
        layer: &PlannedLayer,
        keys: (&KeyInterner, &KeyInterner),
        key: KeyId,
    ) -> Option<KeyId> {
        let pos = key.0 as usize;
        if let Some(&copied) = layer.copy.get(pos) {
            return copied;
        }
        let cache = &mut self.dynamic[index];
        let slot = pos - layer.copy.len();
        if slot >= cache.len() {
            cache.resize(slot + 1, DynamicKey::Unresolved);
        }
        if let DynamicKey::Resolved(resolved) = cache[slot] {
            return resolved;
        }
        let (table, output) = keys;
        let resolved = table.name(key).and_then(|name| match &layer.attributes {
            AttributesDef::All => Some(output.intern(&name)),
            AttributesDef::None => None,
            AttributesDef::Columns(_) => output.get(&name),
        });
        cache[slot] = DynamicKey::Resolved(resolved);
        resolved
    }
}

fn render_batch(
    batch: &mut FeatureBatch,
    worker: &mut Worker,
    plan: &Plan,
    keys: &Keys,
    order: TileOrder,
    buffer: &mut SortBuffer<'_>,
) -> TileGenResult<()> {
    let table = usize::from(batch.table);
    let layers = plan
        .tables
        .get(table)
        .ok_or(TileGenError::UnknownTable(batch.table))?
        .layers
        .clone();
    for (row, feature) in (batch.first_row..).zip(&mut batch.features) {
        let geom = project_geometry(batch.crs, &mut feature.geometry);
        let seq = Seq::new(batch.partition, row)?;
        for index in layers.clone() {
            let layer = &plan.layers[index];
            if layer.geometry.is_some_and(|kind| !kind.matches(geom)) {
                continue;
            }
            worker.props.clear();
            for (key, value) in &feature.props {
                let output = (&keys.tables[table], &keys.layers[index]);
                if let Some(key) = worker.output_key(index, layer, output, *key) {
                    worker.props.push(key, value.as_ref());
                }
            }
            let id = match layer.id {
                IdDef::Keep => feature.id,
                IdDef::Drop => None,
            };
            worker.renderer.render(
                order,
                &layer.render,
                seq,
                &Feature {
                    id,
                    geom,
                    props: &worker.props,
                    zooms: layer.zooms.clone(),
                    simplify: layer.simplify,
                    min_size: layer.min_size,
                },
                buffer,
            )?;
        }
    }
    Ok(())
}

fn project_geometry(crs: Crs, geometry: &mut Geometry) -> FeatureGeom<'_> {
    match geometry {
        Geometry::Points(points) => {
            project_coords(crs, points);
            FeatureGeom::Points(points)
        }
        Geometry::Lines(lines) => {
            for line in lines.iter_mut() {
                project_coords(crs, &mut line.0);
            }
            FeatureGeom::Lines(lines)
        }
        Geometry::Polygons(polygons) => {
            for polygon in polygons.iter_mut() {
                polygon.exterior_mut(|ring| project_coords(crs, &mut ring.0));
                polygon.interiors_mut(|rings| {
                    for ring in rings {
                        project_coords(crs, &mut ring.0);
                    }
                });
            }
            FeatureGeom::Polygons(polygons)
        }
    }
}

fn project_coords(crs: Crs, coords: &mut [Coord<f64>]) {
    match crs {
        Crs::Wgs84 => project::from_lonlat(coords),
        Crs::WebMercator => project::from_mercator(coords),
    }
}

fn complete_metadata(
    metadata: &mut TileJSON,
    infos: &[LayerInfo],
    stats: &[LayerStats],
    format: TileFormat,
) {
    let zooms = stats.iter().filter_map(|s| s.zooms);
    metadata.minzoom = zooms.clone().map(|z| z.0).min();
    metadata.maxzoom = zooms.map(|z| z.1).max();
    let format = match format {
        TileFormat::Mlt(_) => "mlt",
        TileFormat::Mvt => "pbf",
    };
    metadata.other.insert("format".to_owned(), format.into());
    metadata.vector_layers = Some(
        infos
            .iter()
            .zip(stats)
            .filter(|(_, stats)| stats.zooms.is_some())
            .map(|(info, stats)| {
                let fields = stats
                    .fields
                    .iter()
                    .map(|(name, &kind)| (name.clone(), field_type(kind).to_owned()));
                let mut layer = VectorLayer::new(info.name.clone(), fields.collect());
                (layer.minzoom, layer.maxzoom) = stats.zooms.unzip();
                layer
            })
            .collect(),
    );
}

/// `vector_layers` field types, as `MBTiles` and `TileJSON` consumers expect them.
fn field_type(kind: PropKind) -> &'static str {
    match kind {
        PropKind::Bool => "Boolean",
        PropKind::Str => "String",
        PropKind::I8
        | PropKind::U8
        | PropKind::I32
        | PropKind::U32
        | PropKind::I64
        | PropKind::U64
        | PropKind::F32
        | PropKind::F64 => "Number",
    }
}
