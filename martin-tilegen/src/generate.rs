//! Runs a whole generation: render every feature into sorted runs, then merge, encode and write tiles.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::{self, ScopedJoinHandle};

use hotpath::wrap::flume::{Receiver, Sender};
use martin_tile_utils::Encoding;
use tilejson::TileJSON;

use crate::phase1::{RenderTotals, RenderWorker};
use crate::phase2::{TileEncoder, TileEncoding};
use crate::pipeline::{self, BatchSender, spawn_named};
use crate::plan::{ExprErrors, Plan};
use crate::progress::{Activity, Progress, Role, Stage, Tracker};
use crate::sort::{SortConfig, Sorter, TileBatches, TileRecords};
use crate::source::{FeatureBatch, FeatureSource, SourceError};
use crate::{EncodedTile, TileFormat, TileGenError, TileGenResult, TileOrder, TileSink};

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
    /// MVT only: polygon rings keep their source start, rather than starting where `ST_AsMVTGeom`'s wagyu starts them
    /// in tiles that get the whole polygon.
    pub mvt_preserve_ring_start: bool,
    /// MVT only: each layer lists every key it declares, used or not, in declaration order, as `ST_AsMVT` does,
    /// then the other keys in the order features first use them.
    pub mvt_keep_unused_keys: bool,
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
    progress.claim()?;
    let order = sink.tile_order();
    progress.set_partitions(source.partitions());
    if let Some(features) = source.estimated_features() {
        progress.set_estimated_features(features);
    }
    progress.enter(Stage::Render);

    let sorter = Sorter::new(config.sort.clone())?;
    let wagyu_ring_starts = !config.mvt_preserve_ring_start && matches!(config.format, TileFormat::Mvt);
    let mut totals = render_all(
        source,
        &sorter,
        plan,
        order,
        config.threads,
        wagyu_ring_starts,
        progress,
    )?;

    let batches = sorter.into_tiles(config.batch_tiles, progress)?;
    let (encoding, mut batches) = TileEncoding::start(
        plan,
        batches,
        config.format,
        config.encoding,
        order,
        config.mvt_keep_unused_keys,
    );
    progress.set_records(batches.records().1);
    progress.enter(Stage::Write);
    let [merge_thread] = progress.track(Role::Merge, 1)[..] else {
        unreachable!("one merge thread is tracked")
    };
    let ((sink, tiles), encoders) = pipeline::run(
        config.threads,
        config.window.max(1),
        progress,
        |submit: &BatchSender<Vec<TileRecords>, _>| {
            merge_thread.set(Activity::Busy);
            let merged = merge_batches(&mut batches, submit, merge_thread, progress);
            merge_thread.set(Activity::Done);
            merged
        },
        TileEncoder::default,
        |encoder, batch| encoding.encode(encoder, batch),
        move |batches| {
            let mut tiles = 0u64;
            sink.write_all(&mut batches.inspect(|batch| {
                if let Ok(batch) = batch {
                    tiles += batch.len() as u64;
                    let bytes = batch.iter().map(|tile| tile.data.len() as u64).sum();
                    progress.add_tiles(batch.len() as u64, bytes);
                }
            }))?;
            Ok((sink, tiles))
        },
    )?;

    progress.enter(Stage::Finish);
    encoding.complete_metadata(&mut metadata, &encoders);
    totals.add_encoder_errors(&encoders);
    sink.finish(&metadata)?;
    progress.enter(Stage::Done);
    Ok(totals.into_summary(plan, tiles))
}

/// Hands every batch to the encoders, `merger` blocked while they have no room.
fn merge_batches(
    batches: &mut TileBatches,
    submit: &BatchSender<Vec<TileRecords>, Vec<EncodedTile>>,
    merger: Tracker<'_>,
    progress: &Progress,
) -> TileGenResult<()> {
    while let Some(batch) = batches.next() {
        let batch = batch?;
        progress.set_merged_records(batches.records().0);
        merger.during(Activity::Blocked, || submit.send(batch))?;
    }
    Ok(())
}

type FeatureTx = Sender<FeatureBatch>;
type FeatureRx = Receiver<FeatureBatch>;

/// Sends `batch` to the render workers, `reader` blocked while they have no room.
fn send_features(tx: &FeatureTx, batch: FeatureBatch, reader: Tracker<'_>) -> Result<(), SourceError> {
    reader.during(Activity::Blocked, || {
        hotpath::measure_block!(
            { "reader_send" },
            tx.send(batch).map_err(|_closed| SourceError::Stopped)
        )
    })
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
        let readers = source.open_readers(threads.min(partitions as usize))?;
        let reader_trackers = progress.track(Role::Read, readers.len());
        let readers: Vec<_> = readers
            .into_iter()
            .zip(reader_trackers)
            .enumerate()
            .map(|(i, (mut reader, tracker))| {
                let tx = tx.clone();
                spawn_named(scope, "reader", i, move || -> TileGenResult<()> {
                    let tx = tx;
                    tracker.set(Activity::Busy);
                    let read = loop {
                        let partition = next_partition.fetch_add(1, Ordering::Relaxed);
                        if partition >= partitions || failed.load(Ordering::Relaxed) {
                            break Ok(());
                        }
                        if let Err(err) = reader.read(partition, plan.table_keys(), &mut |batch| {
                            send_features(&tx, batch, tracker)
                        }) {
                            break Err(fail(err.into()));
                        }
                        progress.add_partition();
                    };
                    tracker.set(Activity::Done);
                    read
                })
            })
            .collect();
        drop(tx);
        let workers: Vec<_> = progress
            .track(Role::Render, threads)
            .into_iter()
            .enumerate()
            .map(|(i, tracker)| {
                let rx = rx.clone();
                spawn_named(scope, "render", i, move || -> TileGenResult<RenderTotals> {
                    let mut worker = RenderWorker::new(plan, sorter, order, None, wagyu_ring_starts);
                    let rendered = (|| {
                        loop {
                            tracker.set(Activity::Idle);
                            let Ok(mut batch) = rx.recv() else { break };
                            tracker.set(Activity::Busy);
                            if failed.load(Ordering::Relaxed) {
                                break;
                            }
                            worker.render_batch(&mut batch)?;
                            progress.add_features(batch.features.len() as u64);
                            progress.set_runs(sorter.spilled());
                        }
                        tracker.set(Activity::Busy);
                        let totals = worker.finish()?;
                        progress.set_runs(sorter.spilled());
                        Ok(totals)
                    })();
                    tracker.set(Activity::Done);
                    rendered.map_err(fail)
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
