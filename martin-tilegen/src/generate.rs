//! Runs a whole generation: render every feature into sorted runs, then merge, encode and write tiles.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::{self, ScopedJoinHandle};

use crate::source::{FeatureBatch, FeatureSource, SourceError};
use hotpath::wrap::flume::{Receiver, Sender};
use martin_tile_utils::Encoding;
use tilejson::TileJSON;

use crate::phase1::{RenderTotals, RenderWorker};
use crate::phase2::{TileEncoder, TileEncoding};
use crate::pipeline::{self, BatchSender, spawn_named};
use crate::plan::{ExprErrors, Plan};
use crate::sort::{SortConfig, Sorter, TileRecords};
use crate::{TileFormat, TileGenError, TileGenResult, TileOrder, TileSink};

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
    /// MVT only: a polygon ring in a tile that gets its whole polygon starts where `ST_AsMVTGeom`'s wagyu starts it.
    pub mvt_first_vertex_like_wagyu: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub features: u64,
    /// Features with an empty point or an empty geometry collection, which have nothing to draw.
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
    let wagyu_ring_starts = config.mvt_first_vertex_like_wagyu && matches!(config.format, TileFormat::Mvt);
    let totals = render_all(
        source,
        &sorter,
        plan,
        order,
        config.threads,
        wagyu_ring_starts,
        progress,
    )?;
    progress.writing.store(true, Ordering::Relaxed);

    let (encoding, batches) =
        TileEncoding::start(plan, sorter, config.format, config.encoding, order, config.batch_tiles)?;
    let ((sink, tiles), encoders) = pipeline::run(
        config.threads,
        config.window.max(1),
        |submit: &BatchSender<Vec<TileRecords>, _>| {
            for batch in batches {
                submit.send(batch?)?;
            }
            Ok(())
        },
        TileEncoder::default,
        |encoder, batch| encoding.encode(encoder, batch),
        move |batches| {
            let mut tiles = 0u64;
            sink.write_all(&mut batches.inspect(|batch| {
                if let Ok(batch) = batch {
                    tiles += batch.len() as u64;
                    progress.tiles.fetch_add(batch.len() as u64, Ordering::Relaxed);
                }
            }))?;
            Ok((sink, tiles))
        },
    )?;

    encoding.complete_metadata(&mut metadata, &encoders);
    sink.finish(&metadata)?;
    Ok(totals.into_summary(plan, tiles))
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
    hotpath::channel!(flume::bounded::<FeatureBatch>(capacity), label = { "features" })
}

fn render_all<S: FeatureSource>(
    source: &S,
    sorter: &Sorter,
    plan: &Plan,
    order: TileOrder,
    threads: usize,
    wagyu_ring_starts: bool,
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
                            .read(partition, plan.table_keys(), &mut |batch| send_features(&tx, batch))
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
                    let mut worker = RenderWorker::new(plan, sorter, order, None, wagyu_ring_starts);
                    for mut batch in rx {
                        if failed.load(Ordering::Relaxed) {
                            break;
                        }
                        worker.render_batch(&mut batch).map_err(fail)?;
                        progress
                            .features
                            .fetch_add(batch.features.len() as u64, Ordering::Relaxed);
                    }
                    worker.finish().map_err(fail)
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
    handle.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}
