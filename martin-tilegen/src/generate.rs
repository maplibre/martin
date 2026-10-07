//! Runs a whole generation: render every feature into sorted runs, then merge, encode and write tiles.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;

use martin_tile_utils::Encoding;
use mlt_core::PropKind;
use tilejson::{TileJSON, VectorLayer};

use crate::pipeline::{self, Submit};
use crate::props::{KeyInterner, KeyNames, PropRef};
use crate::source::{Crs, FeatureBatch, FeatureSource, Geometry, LayerSpec};
use crate::{
    DedupIndex, EncodeSettings, Feature, FeatureGeom, LayerInfo, LayerStats, RenderLayer, Renderer,
    Seq, SortConfig, Sorter, TileEncoder, TileFormat, TileGenError, TileGenResult, TileGrouper,
    TileRecords, TileSink, project,
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

/// Generates every tile of `source` into `sink`. `metadata` is completed with the zooms, `format` and
/// `vector_layers` the run produced, then stored once all tiles are written.
pub fn generate<S: FeatureSource, K: TileSink>(
    source: &S,
    mut sink: K,
    config: &GenerateConfig,
    mut metadata: TileJSON,
) -> TileGenResult<Summary> {
    let specs = source.layers();
    if specs.len() > 256 {
        return Err(TileGenError::TooManyLayers(specs.len()));
    }
    let order = sink.tile_order();
    let render_layers = specs
        .iter()
        .zip(0u8..)
        .map(|(spec, index)| {
            let mut layer = RenderLayer::new(index, spec.zooms.clone(), spec.grid)?;
            layer.clip = spec.clip;
            Ok(layer)
        })
        .collect::<TileGenResult<Vec<_>>>()?;
    let keys: Vec<_> = specs
        .iter()
        .map(|spec| KeyInterner::new(&spec.known_keys))
        .collect();

    let sorter = Sorter::new(config.sort.clone())?;
    let (features, slice_errors) = render_all(
        source,
        &sorter,
        &render_layers,
        &keys,
        order,
        config.threads,
    )?;

    let names: Vec<KeyNames> = keys.into_iter().map(KeyInterner::freeze).collect();
    let infos: Vec<_> = specs
        .iter()
        .map(|spec| LayerInfo {
            name: spec.name.clone(),
            grid: spec.grid,
            order: spec.order,
        })
        .collect();
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
                }
            }))?;
            Ok((sink, tiles))
        },
    )?;

    let mut layers = vec![LayerStats::default(); specs.len()];
    for encoder in &encoders {
        for (total, stats) in layers.iter_mut().zip(&encoder.stats) {
            total.merge(stats);
        }
    }
    complete_metadata(&mut metadata, specs, &layers, config.format);
    sink.finish(&metadata)?;
    Ok(Summary {
        features,
        tiles,
        slice_errors,
        layers,
    })
}

/// Phase 1: partition readers feed render workers, each spilling into its own sort buffer.
fn render_all<S: FeatureSource>(
    source: &S,
    sorter: &Sorter,
    layers: &[RenderLayer],
    keys: &[KeyInterner],
    order: crate::TileOrder,
    threads: usize,
) -> TileGenResult<(u64, u64)> {
    let threads = threads.max(1);
    let partitions = source.partitions();
    let next_partition = AtomicU32::new(0);
    // Set on the first error so readers and workers stop early instead of finishing the run.
    let failed = AtomicBool::new(false);
    let (tx, rx) = flume::bounded::<FeatureBatch>(threads * 2);
    let fail = |err| {
        failed.store(true, Ordering::Relaxed);
        err
    };

    thread::scope(|scope| {
        let readers: Vec<_> = (0..threads.min(partitions as usize))
            .map(|_| {
                let tx = tx.clone();
                scope.spawn(|| -> TileGenResult<()> {
                    let tx = tx;
                    loop {
                        let partition = next_partition.fetch_add(1, Ordering::Relaxed);
                        if partition >= partitions || failed.load(Ordering::Relaxed) {
                            return Ok(());
                        }
                        source
                            .read(partition, keys, &mut |batch| {
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
                scope.spawn(|| -> TileGenResult<(u64, u64)> {
                    let mut buffer = sorter.buffer();
                    let mut renderer = Renderer::default();
                    let mut count = 0;
                    for mut batch in rx {
                        if failed.load(Ordering::Relaxed) {
                            break;
                        }
                        render_batch(&mut batch, &mut renderer, layers, order, &mut buffer)
                            .map_err(fail)?;
                        count += batch.features.len() as u64;
                    }
                    buffer.finish().map_err(fail)?;
                    Ok((count, renderer.slice_errors))
                })
            })
            .collect();
        drop(rx);

        let mut errors = Vec::new();
        for reader in readers {
            if let Err(err) = reader
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            {
                errors.push(err);
            }
        }
        let mut totals = (0, 0);
        for worker in workers {
            match worker
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            {
                Ok((count, skipped)) => totals = (totals.0 + count, totals.1 + skipped),
                Err(err) => errors.push(err),
            }
        }
        // A reader whose workers failed reports `WriterStopped`; the workers' error is the cause.
        match errors
            .into_iter()
            .min_by_key(|err| matches!(err, TileGenError::WriterStopped))
        {
            Some(err) => Err(err),
            None => Ok(totals),
        }
    })
}

fn render_batch(
    batch: &mut FeatureBatch,
    renderer: &mut Renderer,
    layers: &[RenderLayer],
    order: crate::TileOrder,
    buffer: &mut crate::SortBuffer<'_>,
) -> TileGenResult<()> {
    let layer = &layers[usize::from(batch.layer)];
    let mut props: Vec<(crate::props::KeyId, PropRef<'_>)> = Vec::new();
    for (row, feature) in (batch.first_row..).zip(&mut batch.features) {
        let geom = match &mut feature.geometry {
            Geometry::Points(points) => {
                project_coords(batch.crs, points);
                FeatureGeom::Points(points)
            }
            Geometry::Lines(lines) => {
                for line in lines.iter_mut() {
                    project_coords(batch.crs, &mut line.0);
                }
                FeatureGeom::Lines(lines)
            }
        };
        props.clear();
        props.extend(
            feature
                .props
                .iter()
                .map(|(key, value)| (*key, value.as_ref())),
        );
        let seq = Seq::new(batch.partition, row)?;
        renderer.render(
            order,
            layer,
            seq,
            &Feature {
                id: feature.id,
                geom,
                props: &props,
            },
            buffer,
        )?;
    }
    Ok(())
}

fn project_coords(crs: Crs, coords: &mut [geo_types::Coord<f64>]) {
    match crs {
        Crs::Wgs84 => project::from_lonlat(coords),
        Crs::WebMercator => project::from_mercator(coords),
    }
}

fn complete_metadata(
    metadata: &mut TileJSON,
    specs: &[LayerSpec],
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
        specs
            .iter()
            .zip(stats)
            .filter(|(_, stats)| stats.zooms.is_some())
            .map(|(spec, stats)| {
                let fields = stats
                    .fields
                    .iter()
                    .map(|(name, &kind)| (name.clone(), field_type(kind).to_owned()));
                let mut layer = VectorLayer::new(spec.name.clone(), fields.collect());
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
