//! Runs a whole generation: render every feature into sorted runs, then merge, encode and write tiles.

use std::borrow::Cow;
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::{self, ScopedJoinHandle};

use geo_types::Coord;
use hotpath::wrap::flume::{Receiver, Sender};

use martin_core::generate::props::{KeyId, KeyInterner, KeyNames, Prop};
use martin_core::generate::source::{Crs, FeatureBatch, FeatureSource, Geometry, SourceError};
use martin_tile_utils::{Encoding, Format};
use mlt_core::PropKind;
use tilejson::{TileJSON, VectorLayer};
use tracing::warn;

use crate::expr::{ExprKeys, ExprRef, FeatureView, Filter, PropSlots, Role, Zoom};
use crate::pipeline::{self, BatchSender, spawn_named};
use crate::plan::{
    AttributesDef, Plan, PlannedAttr, PlannedAttrs, PlannedId, PlannedLayer, PlannedRule,
    PlannedValue, PlannedZoom,
};
use crate::record::EncodedProps;
use crate::{
    DedupIndex, EncodeSettings, Feature, FeatureGeom, LayerInfo, LayerStats, Renderer, Seq,
    SliceErrors, SortBuffer, SortConfig, Sorter, TileEncoder, TileFormat, TileGenError,
    TileGenResult, TileGrouper, TileOrder, TileRecords, TileSink, project,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExprErrors {
    pub layer: String,
    pub expr: String,
    pub errors: u64,
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
    let keys = Keys::new(plan);

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
        tiles,
        slice_errors: layer_slice_errors(plan, &totals.slice_errors),
        expr_errors: plan
            .exprs
            .iter()
            .zip(totals.expr_errors)
            .filter(|(_, errors)| *errors > 0)
            .map(|(expr, errors)| ExprErrors {
                layer: plan.layers[expr.layer].info.name.clone(),
                expr: expr.source.clone(),
                errors,
            })
            .collect(),
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

/// Property keys as sources intern them, per table, and as tiles hold them, per output layer.
struct Keys {
    tables: Vec<KeyInterner>,
    /// What expressions know of each table's keys before any feature is read.
    names: Vec<(ExprKeys, u32)>,
    layers: Vec<KeyInterner>,
}

impl Keys {
    fn new(plan: &Plan) -> Self {
        let tables: Vec<_> = plan
            .tables
            .iter()
            .map(|table| {
                let keys = KeyInterner::new(&table.columns);
                for name in &table.reads {
                    keys.intern(name);
                }
                keys
            })
            .collect();
        Self {
            names: plan
                .tables
                .iter()
                .zip(&tables)
                .map(|(table, keys)| {
                    let names = ExprKeys::columns(&table.columns);
                    let mut names = (names, key_count(table.columns.len()));
                    learn_all(&mut names, keys);
                    names
                })
                .collect(),
            tables,
            layers: plan
                .layers
                .iter()
                .map(|layer| KeyInterner::new(&layer.keys))
                .collect(),
        }
    }
}

#[derive(Default)]
struct RenderTotals {
    features: u64,
    /// By layer index.
    slice_errors: Vec<SliceErrors>,
    /// By expression of the plan.
    expr_errors: Vec<u64>,
}

impl RenderTotals {
    fn add(&mut self, worker: Self) {
        self.features += worker.features;
        if self.slice_errors.len() < worker.slice_errors.len() {
            self.slice_errors
                .resize_with(worker.slice_errors.len(), SliceErrors::default);
        }
        for (total, errors) in self.slice_errors.iter_mut().zip(worker.slice_errors) {
            total.merge(errors);
        }
        for (total, errors) in self.expr_errors.iter_mut().zip(worker.expr_errors) {
            *total += errors;
        }
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
    keys: &Keys,
    order: TileOrder,
    threads: usize,
    progress: &Progress,
) -> TileGenResult<RenderTotals> {
    let threads = threads.max(1);
    let partitions = source.partitions();
    let next_partition = AtomicU32::new(0);
    let failed = AtomicBool::new(false);
    let (tx, rx) = feature_channel(threads * 2);
    let warned: Vec<AtomicBool> = plan.exprs.iter().map(|_| AtomicBool::new(false)).collect();
    let run = Run {
        plan,
        keys,
        order,
        warned: &warned,
    };
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
                            .read(partition, &keys.tables, &mut |batch| {
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
                    let mut worker = Worker::new(plan, keys);
                    let mut count = 0;
                    for mut batch in rx {
                        if failed.load(Ordering::Relaxed) {
                            break;
                        }
                        render_batch(&mut batch, &mut worker, &run, &mut buffer).map_err(fail)?;
                        count += batch.features.len() as u64;
                        progress
                            .features
                            .fetch_add(batch.features.len() as u64, Ordering::Relaxed);
                    }
                    buffer.finish().map_err(fail)?;
                    Ok(RenderTotals {
                        features: count,
                        slice_errors: worker.renderer.slice_errors,
                        expr_errors: worker.errors,
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
        let mut totals = RenderTotals {
            expr_errors: vec![0; plan.exprs.len()],
            ..RenderTotals::default()
        };
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

struct Worker {
    renderer: Renderer,
    props: EncodedProps,
    /// The props of each zoom band of the current layer, if it has bands.
    bands: Vec<(u8, EncodedProps)>,
    /// The `sort_by` key of the current feature in the current layer.
    sort: Vec<u8>,
    /// Output key of each table key interned after the declared columns, by layer; filled lazily.
    dynamic: Vec<Vec<DynamicKey>>,
    slots: PropSlots,
    /// Key names expressions read, by table, and how many of the table's keys they hold.
    names: Vec<(ExprKeys, u32)>,
    /// Failed evaluations by expression of the plan.
    errors: Vec<u64>,
}

impl Worker {
    fn new(plan: &Plan, keys: &Keys) -> Self {
        Self {
            renderer: Renderer::default(),
            props: EncodedProps::default(),
            bands: Vec::new(),
            sort: Vec::new(),
            dynamic: vec![Vec::new(); plan.layers.len()],
            slots: PropSlots::default(),
            names: keys.names.clone(),
            errors: vec![0; plan.exprs.len()],
        }
    }
}

#[derive(Clone, Copy)]
enum DynamicKey {
    Unresolved,
    Resolved(Option<KeyId>),
}

fn key_count(columns: usize) -> u32 {
    u32::try_from(columns).expect("fewer than 2^32 columns")
}

fn output_key(
    cache: &mut Vec<DynamicKey>,
    layer: &PlannedLayer,
    keys: (&KeyInterner, &KeyInterner),
    key: KeyId,
) -> Option<KeyId> {
    let pos = key.index();
    if let Some(&copied) = layer.copy.get(pos) {
        return copied;
    }
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
        AttributesDef::None | AttributesDef::Computed { .. } => None,
        AttributesDef::Columns(selected) => selected
            .contains(&name)
            .then(|| output.get(&name))
            .flatten(),
    });
    cache[slot] = DynamicKey::Resolved(resolved);
    resolved
}

fn learn_all(names: &mut (ExprKeys, u32), table: &KeyInterner) {
    let (names, known) = names;
    while let Some(name) = table.name(KeyId::new(*known)) {
        names.insert(&name, KeyId::new(*known));
        *known += 1;
    }
}

/// Makes the table's keys interned since the last feature known to expressions.
fn learn_keys(names: &mut (ExprKeys, u32), props: &[(KeyId, Prop)], table: &KeyInterner) {
    let (names, known) = names;
    for (key, _) in props {
        while *known <= key.get() {
            let id = KeyId::new(*known);
            if let Some(name) = table.name(id) {
                names.insert(&name, id);
            }
            *known += 1;
        }
    }
}

struct Run<'a> {
    plan: &'a Plan,
    keys: &'a Keys,
    order: TileOrder,
    /// Whether an expression has logged its first failure.
    warned: &'a [AtomicBool],
}

impl Run<'_> {
    /// A failed evaluation or conversion is counted, logged the first time, and taken as `null`.
    fn eval<R: Role>(
        &self,
        expr: &ExprRef<R>,
        view: &FeatureView<'_, String>,
        errors: &mut [u64],
    ) -> Option<R::Out> {
        let index = expr.index;
        let planned = &self.plan.exprs[index];
        match planned.expr.eval(view).and_then(|v| expr.role.coerce(v)) {
            Ok(value) => value,
            Err(err) => {
                errors[index] += 1;
                if !self.warned[index].swap(true, Ordering::Relaxed) {
                    warn!(
                        "layer `{}`: `{}` failed and counts as null: {err}",
                        self.plan.layers[planned.layer].info.name, planned.source
                    );
                }
                None
            }
        }
    }

    fn holds(
        &self,
        expr: ExprRef<Filter>,
        view: &FeatureView<'_, String>,
        errors: &mut [u64],
    ) -> bool {
        self.eval(&expr, view, errors).unwrap_or(false)
    }

    /// The layer's zooms for this feature and the first rule it matches, or `None` when the layer
    /// filters the feature out.
    fn feature_zooms<'p>(
        &self,
        layer: &'p PlannedLayer,
        view: &FeatureView<'_, String>,
        errors: &mut [u64],
    ) -> Option<(RangeInclusive<u8>, Option<&'p PlannedRule>)> {
        if let Some(filter) = &layer.filter
            && !self.holds(*filter, view, errors)
        {
            return None;
        }
        let rule = layer.rules.iter().find(|rule| {
            rule.when
                .as_ref()
                .is_none_or(|when| self.holds(*when, view, errors))
        });
        let mut zoom = |rule: Option<&PlannedZoom>, expr: Option<&ExprRef<Zoom>>| {
            let mut eval = |expr| self.eval(expr, view, errors);
            match rule {
                Some(PlannedZoom::Fixed(zoom)) => Some(*zoom),
                Some(PlannedZoom::Expr(rule)) => eval(rule).or_else(|| eval(expr?)),
                None => eval(expr?),
            }
        };
        let min = zoom(
            rule.and_then(|r| r.minzoom.as_ref()),
            layer.minzoom.as_ref(),
        )
        .unwrap_or(*layer.zooms.start());
        let max = zoom(
            rule.and_then(|r| r.maxzoom.as_ref()),
            layer.maxzoom.as_ref(),
        )
        .unwrap_or(*layer.zooms.end());
        (min <= max).then_some((min..=max, rule))
    }

    /// Adds the computed attributes to `props`, and fills `bands` if they have zoom bands.
    fn computed(
        &self,
        attrs: &PlannedAttrs,
        feature: &[(KeyId, Prop)],
        view: Option<&FeatureView<'_, String>>,
        errors: &mut [u64],
        props: &mut EncodedProps,
        bands: &mut Vec<(u8, EncodedProps)>,
    ) {
        for attr in &attrs.computed {
            if let Some(value) = self.attr(attr, feature, view, errors) {
                props.push(attr.key, value.as_ref());
            }
        }
        if attrs.bands.is_empty() {
            return;
        }
        bands.resize_with(attrs.bands.len(), Default::default);
        for (band, &from) in bands.iter_mut().zip(&attrs.bands) {
            band.0 = from;
            band.1.copy_from(props);
        }
        for attr in &attrs.banded {
            let Some(value) = self.attr(attr, feature, view, errors) else {
                continue;
            };
            for (from, band) in bands.iter_mut() {
                if attr.zooms.contains(from) {
                    band.push(attr.key, value.as_ref());
                }
            }
        }
    }

    /// The layer's `sort_by` keys, concatenated; a failed one sorts as `null`.
    fn sort_key(
        &self,
        layer: &PlannedLayer,
        view: &FeatureView<'_, String>,
        errors: &mut [u64],
        out: &mut Vec<u8>,
    ) {
        out.clear();
        for &(expr, descending) in &layer.sort_by {
            if self
                .eval(expr, view, errors, |v| {
                    v.encode_sort(descending, out).map(Some)
                })
                .is_none()
            {
                ExprValue::Null
                    .encode_sort(descending, out)
                    .expect("null has a sort key");
            }
        }
    }

    fn attr<'v>(
        &self,
        attr: &'v PlannedAttr,
        feature: &'v [(KeyId, Prop)],
        view: Option<&'v FeatureView<'_, String>>,
        errors: &mut [u64],
    ) -> Option<Prop<Cow<'v, str>>> {
        let borrowed = |value: &'v Prop| match value {
            Prop::Bool(v) => Prop::Bool(*v),
            Prop::I64(v) => Prop::I64(*v),
            Prop::F32(v) => Prop::F32(*v),
            Prop::F64(v) => Prop::F64(*v),
            Prop::Str(v) => Prop::Str(Cow::Borrowed(v.as_str())),
        };
        match &attr.value {
            PlannedValue::Literal(value) => Some(borrowed(value)),
            PlannedValue::Column(key) => feature
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, value)| borrowed(value)),
            PlannedValue::Expr(expr) => self.eval(expr, view?, errors),
        }
    }
}

#[hotpath::measure]
fn render_batch(
    batch: &mut FeatureBatch,
    worker: &mut Worker,
    run: &Run<'_>,
    buffer: &mut SortBuffer<'_>,
) -> TileGenResult<()> {
    let (plan, keys) = (run.plan, run.keys);
    let table = usize::from(batch.table);
    let planned = plan
        .tables
        .get(table)
        .ok_or(TileGenError::UnknownTable(batch.table))?;
    let Worker {
        renderer,
        props,
        bands,
        sort,
        dynamic,
        slots,
        names,
        errors,
    } = worker;
    for (row, feature) in (batch.first_row..).zip(&mut batch.features) {
        let geom = project_geometry(batch.crs, &mut feature.geometry);
        let seq = Seq::new(batch.partition, row)?;
        if planned.evaluates && planned.dynamic_props {
            learn_keys(&mut names[table], &feature.props, &keys.tables[table]);
        }
        let view = planned
            .evaluates
            .then(|| slots.bind(&names[table].0, &feature.props));
        let view = view.as_ref();
        for index in planned.layers.clone() {
            let layer = &plan.layers[index];
            if layer.geometry.is_some_and(|kind| !kind.matches(geom)) {
                continue;
            }
            let (zooms, rule) = match view {
                Some(view) => match run.feature_zooms(layer, view, errors) {
                    Some(matched) => matched,
                    None => continue,
                },
                None => (layer.zooms.clone(), None),
            };
            let attrs = rule
                .and_then(|rule| rule.attrs.as_ref())
                .unwrap_or(&layer.attrs);
            props.clear();
            for (key, value) in &feature.props {
                let output = (&keys.tables[table], &keys.layers[index]);
                if let Some(key) = output_key(&mut dynamic[index], layer, output, *key)
                    && (attrs.overrides.is_empty() || attrs.overrides.binary_search(&key).is_err())
                {
                    props.push(key, value.as_ref());
                }
            }
            run.computed(attrs, &feature.props, view, errors, props, bands);
            let id = match layer.id {
                PlannedId::Keep => feature.id,
                PlannedId::Drop => None,
                PlannedId::Expr(expr) => view.and_then(|view| run.eval(&expr, view, errors)),
            };
            let sort_key = match view {
                Some(view) if !layer.sort_by.is_empty() => {
                    run.sort_key(layer, view, errors, sort);
                    Some(sort.as_slice())
                }
                _ => None,
            };
            let (props, bands): (&EncodedProps, &[(u8, EncodedProps)]) = match bands.split_first() {
                Some((first, rest)) if !attrs.bands.is_empty() => (&first.1, rest),
                _ => (props, &[]),
            };
            renderer.render(
                run.order,
                &layer.render,
                seq,
                &Feature {
                    id,
                    geom,
                    props,
                    bands,
                    sort: sort_key,
                    zooms,
                    simplify: rule.map_or(layer.simplify, |rule| rule.simplify),
                    min_size: rule.map_or(layer.min_size, |rule| rule.min_size),
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
