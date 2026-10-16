//! Runs a whole generation: render every feature into sorted runs, then merge, encode and write tiles.
//! The two phases follow Planetiler by Mike Barry (<https://github.com/onthegomap/planetiler>, Apache-2.0).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::{self, ScopedJoinHandle};

use geo_types::Coord;
use hotpath::wrap::flume::{Receiver, Sender};
use martin_core::generate::source::{Crs, FeatureBatch, FeatureSource, Geometry, SourceError};
use martin_tile_utils::{Encoding, Format};
use mlt_core::PropKind;
use tilejson::{TileJSON, VectorLayer};

use crate::pipeline::{self, BatchSender, spawn_named};
use crate::plan::{Evaluator, ExprErrors, ExprTallies, Plan};
use crate::{
    DedupIndex, EncodeSettings, FeatureGeom, LayerInfo, LayerStats, Renderer, Seq, SliceErrors,
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
    /// Features whose geometry the generator cannot render: geometry collections and empty geometries.
    pub skipped: u64,
    pub tiles: u64,
    /// Layers with feature-zooms the slicer could not handle and skipped, in plan order.
    pub slice_errors: Vec<LayerSliceErrors>,
    /// Expressions that failed on some features, which then took them as `null`, in plan order.
    pub expr_errors: Vec<ExprErrors>,
}

/// The skipped feature-zooms of one layer, and the first of them in source order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerSliceErrors {
    pub layer: String,
    pub errors: u64,
    pub zoom: u8,
    pub id: Option<u64>,
    pub partition: u32,
    pub row: u64,
    pub error: String,
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

    let sorter = Sorter::new(config.sort.clone())?;
    let totals = render_all(source, &sorter, plan, order, config.threads, progress)?;
    progress.writing.store(true, Ordering::Relaxed);

    let names = plan.key_names();
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
        |submit: &BatchSender<Vec<TileRecords>, _>| loop {
            let mut batch = Vec::with_capacity(batch_tiles);
            hotpath::measure_block!({ "group_batch" }, {
                while batch.len() < batch_tiles {
                    let mut tile = TileRecords::default();
                    if !grouper.next_tile(&mut tile)? {
                        break;
                    }
                    batch.push(tile);
                }
            });
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
        skipped: totals.skipped,
        tiles,
        slice_errors: layer_slice_errors(plan, &totals.slice_errors),
        expr_errors: plan.expr_errors(&totals.exprs),
    })
}

fn layer_slice_errors(plan: &Plan, totals: &[SliceErrors]) -> Vec<LayerSliceErrors> {
    plan.layers
        .iter()
        .filter_map(|layer| {
            let index = usize::from(layer.render.index);
            let errors = totals.get(index)?;
            let first = errors.first.as_ref()?;
            Some(LayerSliceErrors {
                layer: layer.info.name.clone(),
                errors: errors.count,
                zoom: first.zoom,
                id: first.id,
                partition: first.seq.partition(),
                row: first.seq.row(),
                error: first.error.to_string(),
            })
        })
        .collect()
}

#[derive(Default)]
struct RenderTotals {
    features: u64,
    skipped: u64,
    /// By layer index.
    slice_errors: Vec<SliceErrors>,
    exprs: ExprTallies,
}

impl RenderTotals {
    fn add(&mut self, worker: Self) {
        self.features += worker.features;
        self.skipped += worker.skipped;
        if self.slice_errors.len() < worker.slice_errors.len() {
            self.slice_errors
                .resize_with(worker.slice_errors.len(), SliceErrors::default);
        }
        for (total, errors) in self.slice_errors.iter_mut().zip(worker.slice_errors) {
            total.merge(errors);
        }
        self.exprs.merge(worker.exprs);
    }
}

type FeatureTx = Sender<FeatureBatch>;
type FeatureRx = Receiver<FeatureBatch>;

fn send_features(tx: &FeatureTx, batch: FeatureBatch) -> Result<(), SourceError> {
    hotpath::measure_block!(
        { "reader_send" },
        tx.send(batch).map_err(|_closed| SourceError::Stopped)
    )
}

fn feature_channel(capacity: usize) -> (FeatureTx, FeatureRx) {
    hotpath::channel!(
        flume::bounded::<FeatureBatch>(capacity),
        label = { "features" }
    )
}

fn render_all<S: FeatureSource>(
    source: &S,
    sorter: &Sorter,
    plan: &Plan,
    order: TileOrder,
    threads: usize,
    progress: &Progress,
) -> TileGenResult<RenderTotals> {
    let threads = threads.max(1);
    let partitions = source.partitions();
    let next_partition = AtomicU32::new(0);
    let failed = AtomicBool::new(false);
    let (tx, rx) = feature_channel(threads * 2);
    let fail = |err| {
        failed.store(true, Ordering::Relaxed);
        err
    };

    let (next_partition, failed, fail) = (&next_partition, &failed, &fail);
    thread::scope(|scope| {
        let readers: Vec<_> = source
            .open_readers(threads.min(partitions as usize))?
            .into_iter()
            .enumerate()
            .map(|(i, mut reader)| {
                let tx = tx.clone();
                spawn_named(scope, "reader", i, move || -> TileGenResult<()> {
                    let tx = tx;
                    loop {
                        let partition = next_partition.fetch_add(1, Ordering::Relaxed);
                        if partition >= partitions || failed.load(Ordering::Relaxed) {
                            return Ok(());
                        }
                        reader
                            .read(partition, plan.table_keys(), &mut |batch| {
                                send_features(&tx, batch)
                            })
                            .map_err(|err| fail(err.into()))?;
                    }
                })
            })
            .collect();
        drop(tx);
        let workers: Vec<_> = (0..threads)
            .map(|i| {
                let rx = rx.clone();
                spawn_named(scope, "render", i, || -> TileGenResult<RenderTotals> {
                    let mut buffer = sorter.buffer();
                    let mut worker = Worker {
                        renderer: Renderer::default(),
                        evaluator: plan.evaluator(),
                        skipped: 0,
                    };
                    let mut count = 0;
                    for mut batch in rx {
                        if failed.load(Ordering::Relaxed) {
                            break;
                        }
                        render_batch(&mut batch, &mut worker, order, &mut buffer).map_err(fail)?;
                        count += batch.features.len() as u64;
                        progress
                            .features
                            .fetch_add(batch.features.len() as u64, Ordering::Relaxed);
                    }
                    buffer.finish().map_err(fail)?;
                    Ok(RenderTotals {
                        features: count - worker.skipped,
                        skipped: worker.skipped,
                        slice_errors: worker.renderer.slice_errors,
                        exprs: worker.evaluator.finish(),
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
                Ok(worker) => totals.add(worker),
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

struct Worker<'p> {
    renderer: Renderer,
    evaluator: Evaluator<'p>,
    skipped: u64,
}

#[hotpath::measure]
fn render_batch(
    batch: &mut FeatureBatch,
    worker: &mut Worker<'_>,
    order: TileOrder,
    buffer: &mut SortBuffer<'_>,
) -> TileGenResult<()> {
    let Worker {
        renderer,
        evaluator,
        skipped,
    } = worker;
    for (row, feature) in (batch.first_row..).zip(&mut batch.features) {
        project_geometry(batch.crs, &mut feature.geometry);
        let Some(geom) = feature_geom(&feature.geometry) else {
            *skipped += 1;
            continue;
        };
        let seq = Seq::new(batch.partition, row)?;
        evaluator.evaluate(batch.table, feature, geom, |evaluated| {
            renderer.render(order, evaluated.layer(), seq, evaluated.feature(), buffer)
        })?;
    }
    Ok(())
}

/// Decodes and projects the geometry in place; one that fails to decode stays WKB.
fn project_geometry(crs: Crs, geometry: &mut Geometry) {
    if !geometry.decode_wkb() {
        return;
    }
    match geometry {
        Geometry::Points(points) => project_coords(crs, points),
        Geometry::Lines(lines) => {
            for line in lines.iter_mut() {
                project_coords(crs, &mut line.0);
            }
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
        }
        Geometry::Wkb(_) => {}
    }
}

fn feature_geom(geometry: &Geometry) -> Option<FeatureGeom<'_>> {
    match geometry {
        Geometry::Points(points) => Some(FeatureGeom::Points(points)),
        Geometry::Lines(lines) => Some(FeatureGeom::Lines(lines)),
        Geometry::Polygons(polygons) => Some(FeatureGeom::Polygons(polygons)),
        Geometry::Wkb(_) => None,
    }
}

fn project_coords(crs: Crs, coords: &mut [Coord<f64>]) {
    match crs {
        Crs::Wgs84 => coords
            .iter_mut()
            .for_each(|c| *c = project::from_lonlat(*c)),
        Crs::WebMercator => coords
            .iter_mut()
            .for_each(|c| *c = project::from_mercator(*c)),
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
    let format = Format::from(format).metadata_format_value();
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
