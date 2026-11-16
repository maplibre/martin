//! Lowers table sources and their `layers` into the tables of a [`martin_tilegen::plan::Plan`].

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::ops::RangeInclusive;

use indexmap::{IndexMap, IndexSet};
use martin_core::tiles::postgres::column_def;
use martin_tilegen::plan::{
    AttributesDef, ColumnDef, ComputedAttr, DerivedPoint, GeometryDef, GeometryType, IdDef, IdOrder, LayerDef, RuleDef,
    SortDef, TableDef, ThinningGrid, ThinningMethod as PlanThinning, TileOp, TileOpDef, ValueDef, ZoomDef,
};
use martin_tilegen::props::Prop;
use martin_tilegen::{ClipMode, LayerGrid, MAX_ZOOM, PixelThreshold};
use mlt_core::encoder::EncoderConfig;
use tilejson::Bounds;
use tracing::warn;

use super::{GenerateError, GenerateResult};
use crate::config::args::OrderByType;
use crate::config::file::postgres::TableInfo;
use crate::config::file::tiling::{
    Attributes, ByZoomRange, Columns, Expr, IdPolicy, Layer, Layers, Limit, Literal, MergeLines, MergeMulti,
    MergePolygons, MultiGeometry, NonEmpty, OutputGeometry, PerFeature, PerTile, Pixels, PropertySelector,
    RuleSettings, RuleThinning, Rules, SortKey, Thinning, ThinningMethod, Value, ValueSpec, Zoom, ZoomRange,
    ZoomSetting,
};
use crate::generate::postgres::ScanTable;

const DEFAULT_EXTENT: u32 = 4096;
const DEFAULT_BUFFER: u32 = 64;

#[derive(Clone, Debug)]
pub struct LowerOptions {
    pub zooms: RangeInclusive<u8>,
    /// WGS84 area to generate, if not the whole world.
    pub bounds: Option<Bounds>,
    /// MLT encoder settings, if not the layer default.
    pub encoder: Option<EncoderConfig>,
    /// The connection's `order_by`, which orders the features of a source without `layers` by their ids. `None`
    /// for a source without ids.
    pub order_by: Option<OrderByType>,
}

/// The table's layers, or one layer named by `layer_id` or `id` without `layers`, as the only source of a run.
/// `None` when the table, or every layer with fixed zooms, has no zoom within the options' zooms.
///
/// # Errors
///
/// Fails when a layer uses a setting `--mode bulk` does not support yet, or layers writing into one output layer
/// disagree on a setting it has once.
pub fn lower_table(id: &str, info: TableInfo, options: &LowerOptions) -> GenerateResult<Option<ScanTable>> {
    let outputs = OutputLayers::new([(id, table_settings(&info))], &options.zooms)?;
    lower_table_with(id, info, options, &outputs)
}

/// The table's layers as [`lower_table`] lowers them, as one of the sources of `outputs`.
///
/// # Errors
///
/// Fails when a layer uses a setting `--mode bulk` does not support yet.
pub fn lower_table_with(
    id: &str,
    info: TableInfo,
    options: &LowerOptions,
    outputs: &OutputLayers,
) -> GenerateResult<Option<ScanTable>> {
    let columns: Vec<ColumnDef> = info
        .properties
        .iter()
        .flatten()
        .filter_map(|(column, _)| Some(column_def(column.clone(), info.property_type(column)?)))
        .collect();
    let options = LowerOptions {
        order_by: options.order_by.filter(|_| info.id_column.is_some()),
        ..options.clone()
    };
    let Some(def) = lower_source(id, &table_settings(&info), columns, &options, outputs)? else {
        return Ok(None);
    };
    Ok(Some(ScanTable {
        id: id.to_owned(),
        info,
        def,
    }))
}

/// The settings of a table source that its layers take their defaults from.
#[must_use]
pub fn table_settings(info: &TableInfo) -> SourceSettings<'_> {
    SourceSettings {
        minzoom: info.minzoom,
        maxzoom: info.maxzoom,
        extent: info.extent,
        buffer: info.buffer,
        clip_geom: info.clip_geom,
        layer_id: info.layer_id.as_deref(),
        layers: info.layers.as_deref(),
    }
}

/// The source settings that layers take their defaults from.
pub struct SourceSettings<'a> {
    /// The lowest zoom of its layers, if set.
    pub minzoom: Option<u8>,
    /// The highest zoom of its layers, if set.
    pub maxzoom: Option<u8>,
    /// The tile extent of its layers, if set.
    pub extent: Option<NonZeroU32>,
    /// The tile buffer of its layers, if set.
    pub buffer: Option<u32>,
    /// Whether its layers clip geometries to the tile, if set.
    pub clip_geom: Option<bool>,
    /// The name of the one layer of a source without `layers`.
    pub layer_id: Option<&'a str>,
    /// Its layers, if it has more than the one default layer.
    pub layers: Option<&'a Layers>,
}

impl SourceSettings<'_> {
    /// The source's zooms within the run's `zooms`, or `None` when it has none of them.
    fn zooms(&self, zooms: &RangeInclusive<u8>) -> Option<RangeInclusive<u8>> {
        let min = self.minzoom.unwrap_or(0).max(*zooms.start());
        let max = self.maxzoom.unwrap_or(u8::MAX).min(*zooms.end());
        (min <= max).then_some(min..=max)
    }

    /// The extent and buffer of `layer`, defaulting to the source's, then to 4096 and 64.
    fn grid(&self, layer: &Layer) -> LayerGrid {
        LayerGrid {
            extent: layer.extent.or(self.extent).map_or(DEFAULT_EXTENT, NonZeroU32::get),
            buffer: layer.buffer.or(self.buffer).unwrap_or(DEFAULT_BUFFER),
        }
    }
}

/// The fixed zooms of `layer` within its source's `zooms`, or `None` when it has none of them.
fn layer_zooms(layer: &Layer, zooms: &RangeInclusive<u8>) -> Option<RangeInclusive<u8>> {
    let fixed = |zoom: Option<&ZoomSetting>| zoom.and_then(ZoomSetting::fixed).map(|z| z.get());
    let min = fixed(layer.minzoom.as_ref()).map_or(*zooms.start(), |z| z.max(*zooms.start()));
    let max = fixed(layer.maxzoom.as_ref()).map_or(*zooms.end(), |z| z.min(*zooms.end()));
    (min <= max).then_some(min..=max)
}

/// The output layers of the sources a run lowers together: what the layers writing into each one share, lowered
/// once for all of them.
#[derive(Debug, Default)]
pub struct OutputLayers(HashMap<String, OutputLayer>);

/// The `sort_by` and per-tile operations of one output layer, as each layer writing into it lowers them.
#[derive(Debug, Default)]
struct OutputLayer {
    /// The `sort_by` of the layers that set it.
    sort_by: Vec<SortDef>,
    /// The operations of the layers that set them, within the zooms of all its layers.
    per_tile: Vec<TileOpDef>,
}

/// A layer of a source, named in errors and warnings.
#[derive(Clone, Copy, Debug)]
struct Definition<'a> {
    /// The source's id.
    source: &'a str,
    /// The layer's key.
    key: &'a str,
}

/// Two layers writing into one output layer that disagree on a setting it has once.
#[derive(thiserror::Error, Debug)]
#[error(
    "layers `{first}` of source `{first_source}` and `{second}` of source `{second_source}` both write into layer \
     `{output}` but {difference}"
)]
pub struct OutputLayerMismatch {
    /// The output layer's name.
    output: String,
    /// The source of the first layer.
    first_source: String,
    /// The key of the first layer.
    first: String,
    /// The source of the second layer.
    second_source: String,
    /// The key of the second layer.
    second: String,
    /// The setting they disagree on.
    difference: Difference,
}

impl OutputLayerMismatch {
    /// The error of `second` disagreeing with `first` on a setting of `output`.
    fn error(output: &str, first: Definition<'_>, second: Definition<'_>, difference: Difference) -> GenerateError {
        GenerateError::OutputLayerMismatch(Box::new(Self {
            output: output.to_owned(),
            first_source: first.source.to_owned(),
            first: first.key.to_owned(),
            second_source: second.source.to_owned(),
            second: second.key.to_owned(),
            difference,
        }))
    }
}

/// What two layers writing into one output layer disagree on.
#[derive(Debug)]
enum Difference {
    /// Both set one key, differently.
    Setting {
        /// The key they set.
        key: &'static str,
    },
    /// Their extents, after defaulting.
    Extent {
        /// The first layer's extent.
        first: u32,
        /// The second layer's extent.
        second: u32,
    },
}

impl std::fmt::Display for Difference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Setting { key } => {
                write!(
                    f,
                    "set `{key}` differently; set it equal on both, or on only one of them"
                )
            }
            Self::Extent { first, second } => {
                write!(f, "with extents {first} and {second}; a layer has one extent")
            }
        }
    }
}

/// A setting an output layer has once, and the first layer that set it.
type Given<'a, T> = Option<(&'a T, Definition<'a>)>;

/// What the layers writing into one output layer set, gathered before any of them is lowered.
struct Gathered<'a> {
    /// The first of them.
    first: Definition<'a>,
    /// Their extent, which they share.
    extent: u32,
    /// The widest of their buffers.
    buffer: u32,
    /// The zooms of those with some zoom in the run.
    zooms: Option<RangeInclusive<u8>>,
    /// The type of the geometry each of them draws, if it has only one.
    draws: Vec<Option<GeometryType>>,
    /// Their `sort_by`.
    sort_by: Given<'a, NonEmpty<SortKey>>,
    /// Their `merge_lines`.
    merge_lines: Given<'a, ByZoomRange<MergeLines>>,
    /// Their `merge_polygons`.
    merge_polygons: Given<'a, ByZoomRange<MergePolygons>>,
    /// Their `thinning`.
    thinning: Given<'a, ByZoomRange<Thinning>>,
    /// Their `merge_multi`.
    merge_multi: Given<'a, ByZoomRange<MergeMulti>>,
    /// Their `limit`.
    limit: Given<'a, ByZoomRange<Limit>>,
}

impl OutputLayers {
    /// The output layers of `sources`, each an id and its settings, rendered at `zooms`.
    ///
    /// # Errors
    ///
    /// If two layers writing into one output layer have different extents, or set `sort_by` or one per-tile operation
    /// differently, or a layer's `geometry` cannot be lowered.
    pub fn new<'a>(
        sources: impl IntoIterator<Item = (&'a str, SourceSettings<'a>)>,
        zooms: &RangeInclusive<u8>,
    ) -> GenerateResult<Self> {
        let mut gathered: IndexMap<&str, Gathered<'_>> = IndexMap::new();
        for (id, source) in sources {
            let Some(layers) = source.layers else {
                continue;
            };
            let source_zooms = source.zooms(zooms);
            for (key, layer) in layers.iter() {
                let definition = Definition { source: id, key };
                let output = layer.layer.as_deref().unwrap_or(key);
                let grid = source.grid(layer);
                let geometry = lower_geometry(key, layer.geometry.as_ref())?;
                let draws = geometry.first().and_then(|entry| entry.draws());
                let layer_zooms = source_zooms.as_ref().and_then(|zooms| layer_zooms(layer, zooms));
                let entry = gathered.entry(output).or_insert_with(|| Gathered {
                    first: definition,
                    extent: grid.extent,
                    buffer: 0,
                    zooms: None,
                    draws: Vec::new(),
                    sort_by: None,
                    merge_lines: None,
                    merge_polygons: None,
                    thinning: None,
                    merge_multi: None,
                    limit: None,
                });
                if grid.extent != entry.extent {
                    let difference = Difference::Extent {
                        first: entry.extent,
                        second: grid.extent,
                    };
                    return Err(OutputLayerMismatch::error(output, entry.first, definition, difference));
                }
                if let Some(layer_zooms) = layer_zooms {
                    entry.buffer = entry.buffer.max(grid.buffer);
                    entry.draws.push(draws);
                    entry.zooms = Some(match entry.zooms.take() {
                        Some(known) => {
                            (*known.start()).min(*layer_zooms.start())..=(*known.end()).max(*layer_zooms.end())
                        }
                        None => layer_zooms,
                    });
                }
                let per_tile = &layer.per_tile;
                gather(
                    &mut entry.sort_by,
                    layer.sort_by.as_ref(),
                    definition,
                    output,
                    "sort_by",
                )?;
                gather(
                    &mut entry.merge_lines,
                    per_tile.merge_lines.as_ref(),
                    definition,
                    output,
                    "merge_lines",
                )?;
                let merge_polygons = per_tile.merge_polygons.as_ref();
                gather(
                    &mut entry.merge_polygons,
                    merge_polygons,
                    definition,
                    output,
                    "merge_polygons",
                )?;
                gather(
                    &mut entry.thinning,
                    per_tile.thinning.as_ref(),
                    definition,
                    output,
                    "thinning",
                )?;
                gather(
                    &mut entry.merge_multi,
                    per_tile.merge_multi.as_ref(),
                    definition,
                    output,
                    "merge_multi",
                )?;
                gather(&mut entry.limit, per_tile.limit.as_ref(), definition, output, "limit")?;
            }
        }
        Ok(Self(
            gathered
                .into_iter()
                .map(|(output, gathered)| (output.to_owned(), gathered.lower()))
                .collect(),
        ))
    }
}

/// Takes `value`, which `definition` sets, as the output layer's `setting`, unless a layer before it set it, which
/// must then have set it equal.
fn gather<'a, T: PartialEq>(
    given: &mut Given<'a, T>,
    value: Option<&'a T>,
    definition: Definition<'a>,
    output: &str,
    setting: &'static str,
) -> GenerateResult<()> {
    let Some(value) = value else {
        return Ok(());
    };
    match given {
        Some((known, first)) if *known != value => Err(OutputLayerMismatch::error(
            output,
            *first,
            definition,
            Difference::Setting { key: setting },
        )),
        Some(_) => Ok(()),
        None => {
            *given = Some((value, definition));
            Ok(())
        }
    }
}

impl Gathered<'_> {
    /// The output layer's settings, each operation lowered within the zooms of all its layers, with warnings naming
    /// the layer that set it.
    fn lower(&self) -> OutputLayer {
        let Some(zooms) = &self.zooms else {
            return OutputLayer::default();
        };
        let grid = LayerGrid {
            extent: self.extent,
            buffer: self.buffer,
        };
        let geometry = self
            .draws
            .iter()
            .all(|draws| *draws == self.draws[0])
            .then_some(self.draws[0])
            .flatten();
        let mut per_tile = Vec::new();
        let mut lower = |definition: Definition<'_>, ops: PerTile| {
            let grids = GridContext {
                source: definition.source,
                layer: definition.key,
                zooms: zooms.clone(),
                grid,
                geometry,
            };
            per_tile.extend(lower_per_tile(
                definition.source,
                definition.key,
                &ops,
                zooms,
                grid,
                geometry,
                &grids,
            ));
        };
        if let Some((op, definition)) = self.merge_lines {
            lower(
                definition,
                PerTile {
                    merge_lines: Some(op.clone()),
                    ..PerTile::default()
                },
            );
        }
        if let Some((op, definition)) = self.merge_polygons {
            lower(
                definition,
                PerTile {
                    merge_polygons: Some(op.clone()),
                    ..PerTile::default()
                },
            );
        }
        if let Some((op, definition)) = self.thinning {
            lower(
                definition,
                PerTile {
                    thinning: Some(op.clone()),
                    ..PerTile::default()
                },
            );
        }
        if let Some((op, definition)) = self.merge_multi {
            lower(
                definition,
                PerTile {
                    merge_multi: Some(op.clone()),
                    ..PerTile::default()
                },
            );
        }
        if let Some((op, definition)) = self.limit {
            lower(
                definition,
                PerTile {
                    limit: Some(op.clone()),
                    ..PerTile::default()
                },
            );
        }
        OutputLayer {
            sort_by: self
                .sort_by
                .iter()
                .flat_map(|(keys, _)| keys.iter())
                .map(|key| SortDef {
                    expr: key.expr.as_str().to_owned(),
                    descending: key.descending,
                })
                .collect(),
            per_tile,
        }
    }
}

/// The source's layers over `columns`, as [`lower_table`] lowers them for any kind of source, as one of the sources
/// of `outputs`.
///
/// # Errors
///
/// Fails when a layer uses a setting `--mode bulk` does not support yet.
pub fn lower_source(
    id: &str,
    source: &SourceSettings<'_>,
    columns: Vec<ColumnDef>,
    options: &LowerOptions,
    outputs: &OutputLayers,
) -> GenerateResult<Option<TableDef>> {
    let Some(zooms) = source.zooms(&options.zooms) else {
        warn!(
            "Skipping source `{id}`: it has no zoom between {} and {}",
            options.zooms.start(),
            options.zooms.end()
        );
        return Ok(None);
    };
    let mut defaults = LayerDef {
        clip_mode: source.clip_geom.map_or(ClipMode::Clipped, clip_mode),
        bounds: options.bounds,
        ..LayerDef::new(
            String::new(),
            zooms,
            LayerGrid {
                extent: source.extent.map_or(DEFAULT_EXTENT, NonZeroU32::get),
                buffer: source.buffer.unwrap_or(DEFAULT_BUFFER),
            },
        )
    };
    if let Some(encoder) = options.encoder {
        defaults.encoder = encoder;
    }
    let static_columns: Vec<&str> = columns
        .iter()
        .filter(|column| !matches!(column, ColumnDef::Keys(_)))
        .map(ColumnDef::name)
        .collect();
    let layers = match source.layers {
        None => vec![LayerDef {
            name: source.layer_id.unwrap_or(id).to_owned(),
            id_order: options.order_by.map(|order| match order {
                OrderByType::IdAsc => IdOrder::Ascending,
                OrderByType::IdDesc => IdOrder::Descending,
            }),
            ..defaults
        }],
        Some(layers) => {
            let mut lowered = Vec::new();
            for (name, layer) in layers.iter() {
                if let Some(def) = lower_layer(id, name, layer, source, &defaults, &static_columns, outputs)? {
                    lowered.push(def);
                }
            }
            lowered
        }
    };
    if layers.is_empty() {
        return Ok(None);
    }
    Ok(Some(TableDef { columns, layers }))
}

fn lower_layer(
    id: &str,
    name: &str,
    layer: &Layer,
    source: &SourceSettings<'_>,
    defaults: &LayerDef,
    static_columns: &[&str],
    outputs: &OutputLayers,
) -> GenerateResult<Option<LayerDef>> {
    let geometry = lower_geometry(name, layer.geometry.as_ref())?;
    let output = geometry.first().and_then(|entry| entry.draws());
    let Some(zooms) = layer_zooms(layer, &defaults.zooms) else {
        warn!(
            "Skipping layer `{name}` of source `{id}`: it has no zoom between {} and {}",
            defaults.zooms.start(),
            defaults.zooms.end()
        );
        return Ok(None);
    };
    let threshold = |below: Option<Pixels>, at: Option<Pixels>, default: PixelThreshold| PixelThreshold {
        below_max_zoom: below.map_or(default.below_max_zoom, Pixels::get),
        at_max_zoom: at.map_or(default.at_max_zoom, Pixels::get),
    };
    let grid = source.grid(layer);
    let grids = GridContext {
        source: id,
        layer: name,
        zooms: zooms.clone(),
        grid,
        geometry: output,
    };
    let shared = outputs
        .0
        .get(layer.layer.as_deref().unwrap_or(name))
        .expect("the output layers were gathered from this source");
    Ok(Some(LayerDef {
        name: name.to_owned(),
        output: layer.layer.clone(),
        zooms,
        grid,
        clip_mode: layer.clip_geom.map_or(defaults.clip_mode, clip_mode),
        simplify: threshold(
            layer.simplify,
            layer.simplify_at_maxzoom(),
            PixelThreshold::DEFAULT_SIMPLIFY,
        ),
        min_size: threshold(
            layer.min_size,
            layer.min_size_at_maxzoom(),
            PixelThreshold::DEFAULT_MIN_SIZE,
        ),
        bounds: defaults.bounds,
        encoder: defaults.encoder,
        geometry,
        filter: layer.r#where.as_ref().map(|e| e.as_str().to_owned()),
        minzoom_expr: zoom_expr(layer.minzoom.as_ref()),
        maxzoom_expr: zoom_expr(layer.maxzoom.as_ref()),
        id: match &layer.id {
            IdPolicy::Keep => IdDef::Keep,
            IdPolicy::Drop => IdDef::Drop,
            IdPolicy::Expr(expr) => IdDef::Expr(expr.as_str().to_owned()),
        },
        attributes: match &layer.attributes {
            Attributes::None => AttributesDef::None,
            Attributes::Properties(selectors) => AttributesDef::Columns(expand(selectors.as_slice(), static_columns)),
            Attributes::AllProperties => AttributesDef::All,
            Attributes::Columns(columns) => computed(columns),
        },
        rules: layer
            .rules
            .as_ref()
            .map_or_else(Vec::new, |rules| lower_rules(rules, &grids)),
        sort_by: shared.sort_by.clone(),
        id_order: None,
        per_tile: shared.per_tile.clone(),
    }))
}

/// What lowering a grid thinning needs to know about its layer, to clamp it and warn about it.
struct GridContext<'a> {
    /// The table source's id.
    source: &'a str,
    /// The layer's name.
    layer: &'a str,
    /// The layer's zooms.
    zooms: RangeInclusive<u8>,
    /// The layer's extent and buffer.
    grid: LayerGrid,
    /// The type of the geometry the layer draws, if it has only one.
    geometry: Option<GeometryType>,
}

impl GridContext<'_> {
    /// A rule's grid `thinning` within the layer's zooms, warning about an entry with none of them, and as
    /// [`Self::check`] does.
    fn lower(&self, thinning: &ByZoomRange<Thinning>, key: &str) -> Vec<ThinningGrid> {
        let Self {
            source, layer, zooms, ..
        } = self;
        let mut lowered = Vec::new();
        for entry in thinning {
            let ThinningMethod::Grid { size, limit } = entry.method else {
                continue;
            };
            let range = entry.zooms;
            let lo = range.minzoom().map_or(*zooms.start(), |z| z.get().max(*zooms.start()));
            let hi = range.maxzoom().map_or(*zooms.end(), |z| z.get().min(*zooms.end()));
            if lo > hi {
                warn!(
                    "Skipping `{key}` of layer `{layer}` in source `{source}`: it has no zoom between {} and {}",
                    zooms.start(),
                    zooms.end()
                );
                continue;
            }
            self.check(key, size.get());
            lowered.push(ThinningGrid {
                zooms: lo..=hi,
                size: size.get(),
                limit,
            });
        }
        lowered
    }

    /// Warns about a grid of `size` px squares on a layer without points, and about a buffer smaller than
    /// the square, which neighboring tiles then count differently.
    fn check(&self, key: &str, size: f64) {
        let Self {
            source,
            layer,
            grid,
            geometry,
            ..
        } = self;
        if matches!(geometry, Some(GeometryType::Line | GeometryType::Polygon)) {
            warn!(
                "`{key}` of layer `{layer}` in source `{source}` only thins points by a grid, and the layer has none"
            );
        }
        let buffer = f64::from(grid.buffer) * 256.0 / f64::from(grid.extent);
        if buffer < size {
            warn!(
                "`{key}` of layer `{layer}` in source `{source}` has {size} px squares but a {buffer} px buffer, so \
                 neighboring tiles may keep different points of a square crossing their edge; set `buffer` to at \
                 least {} to avoid it",
                (size * f64::from(grid.extent) / 256.0).ceil()
            );
        }
    }
}

/// The rules in order, then the catch-all; a rule's `simplify` and `min_size` also apply at the max zoom.
fn lower_rules(rules: &Rules, grids: &GridContext<'_>) -> Vec<RuleDef> {
    let lower = |when: Option<&Expr>, settings: &RuleSettings| {
        let zoom = |zoom: Option<&ZoomSetting>| {
            zoom.map(|zoom| match zoom {
                PerFeature::Fixed(zoom) => ZoomDef::Fixed(zoom.get()),
                PerFeature::Expr(expr) => ZoomDef::Expr(expr.as_str().to_owned()),
            })
        };
        let threshold = |pixels: Option<Pixels>| {
            pixels.map(|pixels| PixelThreshold {
                below_max_zoom: pixels.get(),
                at_max_zoom: pixels.get(),
            })
        };
        RuleDef {
            when: when.map(|expr| expr.as_str().to_owned()),
            minzoom: zoom(settings.minzoom.as_ref()),
            maxzoom: zoom(settings.maxzoom.as_ref()),
            simplify: threshold(settings.simplify),
            min_size: threshold(settings.min_size),
            attributes: computed_attrs(&settings.attributes),
            thinning: settings.thinning.as_ref().map(|thinning| match thinning {
                RuleThinning::Off => Vec::new(),
                RuleThinning::Grid(thinning) => grids.lower(thinning, "rules[].thinning"),
            }),
        }
    };
    rules
        .cases
        .iter()
        .map(|rule| lower(Some(&rule.when), &rule.settings))
        .chain(rules.fallback.iter().map(|settings| lower(None, settings)))
        .collect()
}

fn zoom_expr(zoom: Option<&ZoomSetting>) -> Option<String> {
    match zoom? {
        PerFeature::Fixed(_) => None,
        PerFeature::Expr(expr) => Some(expr.as_str().to_owned()),
    }
}

fn computed(columns: &Columns) -> AttributesDef {
    AttributesDef::Computed {
        attributes: computed_attrs(&columns.computed),
        prefixes: columns.copied_prefixes.clone(),
    }
}

fn computed_attrs(attributes: &IndexMap<String, ValueSpec>) -> Vec<ComputedAttr> {
    attributes
        .iter()
        .map(|(name, spec)| ComputedAttr {
            name: name.clone(),
            value: match &spec.value {
                Value::Literal(Literal::Bool(v)) => ValueDef::Literal(Prop::Bool(*v)),
                Value::Literal(Literal::Int(v)) => ValueDef::Literal(Prop::I64(*v)),
                Value::Literal(Literal::Float(v)) => ValueDef::Literal(Prop::F64(v.get())),
                Value::Literal(Literal::String(v)) => ValueDef::Literal(Prop::Str(v.clone())),
                Value::Expr(expr) => ValueDef::Expr(expr.as_str().to_owned()),
            },
            zooms: spec.zooms.minzoom().map_or(0, Zoom::get)..=spec.zooms.maxzoom().map_or(MAX_ZOOM, Zoom::get),
        })
        .collect()
}

/// The entries of the layer's `geometry`, refusing a value the engine cannot derive yet, entries drawing
/// different geometry types, and an entry whose types the entries before it all take.
fn lower_geometry(layer: &str, geometry: Option<&NonEmpty<OutputGeometry>>) -> GenerateResult<Vec<GeometryDef>> {
    let Some(geometry) = geometry else {
        return Ok(Vec::new());
    };
    let list = || {
        let names: Vec<_> = geometry.iter().map(|entry| entry.name()).collect();
        names.join(", ")
    };
    let mut lowered: Vec<GeometryDef> = Vec::new();
    for &entry in geometry {
        let def = layer_geometry(entry).ok_or_else(|| GenerateError::Unsupported {
            layer: layer.to_owned(),
            what: format!("`geometry: {}`", entry.name()),
        })?;
        if lowered.first().is_some_and(|first| first.draws() != def.draws()) {
            return Err(GenerateError::MixedGeometry {
                layer: layer.to_owned(),
                geometry: list(),
            });
        }
        let taken = |kind: GeometryType| lowered.iter().any(|before| before.takes.is_none_or(|t| t == kind));
        let kinds = def.takes.map_or(
            vec![GeometryType::Point, GeometryType::Line, GeometryType::Polygon],
            |kind| vec![kind],
        );
        if kinds.into_iter().all(taken) {
            return Err(GenerateError::UnreachableGeometry {
                layer: layer.to_owned(),
                entry: entry.name(),
                geometry: list(),
            });
        }
        lowered.push(def);
    }
    Ok(lowered)
}

/// The features `geometry` takes and the point it derives from each, as Planetiler's custommap pairs
/// them; `None` for a value the engine cannot derive yet.
fn layer_geometry(geometry: OutputGeometry) -> Option<GeometryDef> {
    let (takes, point) = match geometry {
        OutputGeometry::Point => (Some(GeometryType::Point), None),
        OutputGeometry::Line => (Some(GeometryType::Line), None),
        OutputGeometry::Polygon => (Some(GeometryType::Polygon), None),
        OutputGeometry::PolygonCentroid => (Some(GeometryType::Polygon), Some(DerivedPoint::Centroid)),
        OutputGeometry::LineCentroid => (Some(GeometryType::Line), Some(DerivedPoint::Centroid)),
        OutputGeometry::Centroid => (None, Some(DerivedPoint::Centroid)),
        OutputGeometry::PolygonCentroidIfConvex => (Some(GeometryType::Polygon), Some(DerivedPoint::CentroidIfConvex)),
        OutputGeometry::PolygonPointOnSurface => (Some(GeometryType::Polygon), Some(DerivedPoint::PointOnSurface)),
        OutputGeometry::LineMidpoint | OutputGeometry::PointOnLine | OutputGeometry::InnermostPoint => return None,
    };
    Some(GeometryDef { takes, point })
}

/// The layer's per-tile operations within `zooms`, warning about each one that has none of them, each
/// one that never acts on the layer's `geometry`, each `buffer` wider than the layer's, which is clamped to
/// it, and a grid thinning as [`GridContext::check`] does.
fn lower_per_tile(
    id: &str,
    name: &str,
    per_tile: &PerTile,
    zooms: &RangeInclusive<u8>,
    grid: LayerGrid,
    geometry: Option<GeometryType>,
    grids: &GridContext<'_>,
) -> Vec<TileOpDef> {
    let layer_buffer = f64::from(grid.buffer) * 256.0 / f64::from(grid.extent);
    let clamp_buffer = |op_name: &str, buffer: Option<Pixels>| {
        buffer.map(|buffer| {
            let buffer = buffer.get();
            if buffer > layer_buffer {
                warn!(
                    "`{op_name}` of layer `{name}` in source `{id}`: its {buffer} px buffer is wider than \
                     the layer's {layer_buffer} px, which already clipped the features, so it uses {layer_buffer} px"
                );
            }
            buffer.min(layer_buffer)
        })
    };
    let mut ops = Vec::new();
    let mut push = |op_name: &str, range: ZoomRange, op: TileOp| {
        let lo = range.minzoom().map_or(*zooms.start(), |z| z.get().max(*zooms.start()));
        let hi = range.maxzoom().map_or(*zooms.end(), |z| z.get().min(*zooms.end()));
        if lo > hi {
            warn!(
                "Skipping `{op_name}` of layer `{name}` in source `{id}`: it has no zoom between {} and {}",
                zooms.start(),
                zooms.end()
            );
            return false;
        }
        ops.push(TileOpDef { zooms: lo..=hi, op });
        true
    };
    for merge in per_tile.merge_lines.iter().flat_map(ByZoomRange::iter) {
        if let Some(geometry) = geometry.filter(|g| *g != GeometryType::Line) {
            warn!(
                "`merge_lines` of layer `{name}` in source `{id}` never acts on its {} features",
                geometry_name(geometry)
            );
        }
        push(
            "merge_lines",
            merge.zooms,
            TileOp::MergeLines {
                min_length: merge.min_length.map_or(0.0, Pixels::get),
                tolerance: merge.tolerance.map_or(0.0, Pixels::get),
                buffer: clamp_buffer("merge_lines", merge.buffer),
            },
        );
    }
    for merge in per_tile.merge_polygons.iter().flat_map(ByZoomRange::iter) {
        if let Some(geometry) = geometry.filter(|g| *g != GeometryType::Polygon) {
            warn!(
                "`merge_polygons` of layer `{name}` in source `{id}` never acts on its {} features",
                geometry_name(geometry)
            );
        }
        push(
            "merge_polygons",
            merge.zooms,
            TileOp::MergePolygons {
                min_area: merge.min_area.map_or(0.0, Pixels::get),
                min_hole_area: merge.min_hole_area.map_or(0.0, Pixels::get),
                distance: merge.distance.map_or(0.0, Pixels::get),
                buffer: clamp_buffer("merge_polygons", merge.buffer),
                group_by: merge.group_by.clone(),
            },
        );
    }
    for thinning in per_tile.thinning.iter().flat_map(ByZoomRange::iter) {
        let (op, grid_size) = lower_thinning(thinning);
        if push("thinning", thinning.zooms, op)
            && let Some(size) = grid_size
        {
            grids.check("thinning", size);
        }
    }
    for merge in per_tile.merge_multi.iter().flat_map(ByZoomRange::iter) {
        let types: Vec<GeometryType> = merge.geometry.as_ref().map_or_else(
            || vec![GeometryType::Point, GeometryType::Line, GeometryType::Polygon],
            |types| types.iter().map(|&g| multi_geometry(g)).collect(),
        );
        if let Some(geometry) = geometry.filter(|g| !types.contains(g)) {
            warn!(
                "`merge_multi` of layer `{name}` in source `{id}` never acts on its {} features",
                geometry_name(geometry)
            );
        }
        push(
            "merge_multi",
            merge.zooms,
            TileOp::MergeMulti {
                geometry: types,
                group_by: merge.group_by.iter().cloned().collect(),
            },
        );
    }
    for limit in per_tile.limit.iter().flat_map(ByZoomRange::iter) {
        push("limit", limit.zooms, TileOp::Limit { count: limit.count });
    }
    ops
}

/// The plan's operation for `thinning`, and the side of its squares if it is a grid.
fn lower_thinning(thinning: &Thinning) -> (TileOp, Option<f64>) {
    let (method, grid_size) = match thinning.method {
        ThinningMethod::Grid { size, limit } => (
            PlanThinning::Grid {
                size: size.get(),
                limit,
            },
            Some(size.get()),
        ),
        ThinningMethod::Distance { distance } => (
            PlanThinning::Distance {
                distance: distance.get(),
            },
            None,
        ),
        ThinningMethod::Tile => (PlanThinning::Tile, None),
    };
    let keys = thinning
        .keys
        .iter()
        .flat_map(|keys| keys.iter())
        .map(|key| key.as_str().to_owned())
        .collect();
    (TileOp::Thinning { method, keys }, grid_size)
}

/// The plan's geometry type for a `merge_multi` `geometry` entry.
fn multi_geometry(geometry: MultiGeometry) -> GeometryType {
    match geometry {
        MultiGeometry::Point => GeometryType::Point,
        MultiGeometry::Line => GeometryType::Line,
        MultiGeometry::Polygon => GeometryType::Polygon,
    }
}

/// The config's name of a geometry type, as `geometry` writes it.
fn geometry_name(geometry: GeometryType) -> &'static str {
    match geometry {
        GeometryType::Point => "point",
        GeometryType::Line => "line",
        GeometryType::Polygon => "polygon",
    }
}

/// `clip_geom: false` hands each tile a line or polygon reaches the whole geometry, as
/// `ST_AsMVTGeom` does when serving.
fn clip_mode(clip_geom: bool) -> ClipMode {
    if clip_geom {
        ClipMode::Clipped
    } else {
        ClipMode::Unclipped
    }
}

/// Named keys in their order, each prefix as the matching table columns in table order; keys inside
/// a `jsonb` column are only selected by name.
fn expand(selectors: &[PropertySelector], static_columns: &[&str]) -> Vec<String> {
    let mut keys = IndexSet::new();
    for selector in selectors {
        match selector {
            PropertySelector::Named(name) => {
                keys.insert(name.clone());
            }
            PropertySelector::Prefixed(prefix) => keys.extend(
                static_columns
                    .iter()
                    .filter(|column| column.starts_with(prefix.as_str()))
                    .map(|&column| column.to_owned()),
            ),
        }
    }
    keys.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    fn lower(yaml: &str, zooms: RangeInclusive<u8>) -> GenerateResult<Option<ScanTable>> {
        let info: TableInfo = serde_saphyr::from_str(yaml).expect("valid table");
        lower_table(
            "roads_source",
            info,
            &LowerOptions {
                zooms,
                bounds: None,
                encoder: None,
                order_by: None,
            },
        )
    }

    #[test]
    fn a_table_without_layers_is_one_layer_named_by_its_layer_id() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            layer_id: roads
            schema: public
            table: roads
            srid: 4326
            geometry_column: geom
            minzoom: 2
            maxzoom: 10
            extent: 512
            buffer: 8
            properties:
              name: text
              rank: int4
        "})
        .expect("valid table");
        let options = LowerOptions {
            zooms: 0..=8,
            bounds: Some(Bounds::new(-10.0, -5.0, 10.0, 5.0)),
            encoder: None,
            order_by: None,
        };
        let scan = lower_table("roads_source", info, &options)
            .expect("lowers")
            .expect("has a layer");
        let mut settings = insta::Settings::clone_current();
        settings.add_filter(r"EncoderConfig \{[^}]*\}", "EncoderConfig { .. }");
        let _guard = settings.bind_to_scope();
        insta::assert_debug_snapshot!(scan.def, @r#"
        TableDef {
            columns: [
                Value(
                    "name",
                    Some(
                        String,
                    ),
                ),
                Value(
                    "rank",
                    Some(
                        Int,
                    ),
                ),
            ],
            layers: [
                LayerDef {
                    name: "roads",
                    output: None,
                    zooms: 2..=8,
                    grid: LayerGrid {
                        extent: 512,
                        buffer: 8,
                    },
                    clip_mode: Clipped,
                    simplify: PixelThreshold {
                        below_max_zoom: 0.1,
                        at_max_zoom: 0.0625,
                    },
                    min_size: PixelThreshold {
                        below_max_zoom: 1.0,
                        at_max_zoom: 0.0625,
                    },
                    bounds: Some(
                        Bounds {
                            left: -10.0,
                            bottom: -5.0,
                            right: 10.0,
                            top: 5.0,
                        },
                    ),
                    encoder: EncoderConfig { .. },
                    geometry: [],
                    filter: None,
                    minzoom_expr: None,
                    maxzoom_expr: None,
                    id: Keep,
                    attributes: All,
                    rules: [],
                    sort_by: [],
                    id_order: None,
                    per_tile: [],
                },
            ],
        }
        "#);
    }

    #[test]
    fn a_table_without_layer_id_names_its_layer_by_the_source_id() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
            "},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        let mut settings = insta::Settings::clone_current();
        settings.add_filter(r"EncoderConfig \{[^}]*\}", "EncoderConfig { .. }");
        let _guard = settings.bind_to_scope();
        insta::assert_debug_snapshot!(scan.def, @r#"
        TableDef {
            columns: [],
            layers: [
                LayerDef {
                    name: "roads_source",
                    output: None,
                    zooms: 0..=14,
                    grid: LayerGrid {
                        extent: 4096,
                        buffer: 64,
                    },
                    clip_mode: Clipped,
                    simplify: PixelThreshold {
                        below_max_zoom: 0.1,
                        at_max_zoom: 0.0625,
                    },
                    min_size: PixelThreshold {
                        below_max_zoom: 1.0,
                        at_max_zoom: 0.0625,
                    },
                    bounds: None,
                    encoder: EncoderConfig { .. },
                    geometry: [],
                    filter: None,
                    minzoom_expr: None,
                    maxzoom_expr: None,
                    id: Keep,
                    attributes: All,
                    rules: [],
                    sort_by: [],
                    id_order: None,
                    per_tile: [],
                },
            ],
        }
        "#);
    }

    #[test]
    #[expect(clippy::too_many_lines)]
    fn each_layer_overrides_the_table() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                minzoom: 1
                extent: 1024
                buffer: 16
                clip_geom: false
                prefetch: 2
                properties:
                  name: text
                  class: text
                layers:
                  roads:
                    geometry: line
                    minzoom: 4
                    maxzoom: 20
                    extent: 4096
                    buffer: 0
                    clip_geom: true
                    simplify: 2
                    min_size_at_maxzoom: 0.5
                    id: false
                  road_points:
                    geometry: point
                    maxzoom: 6
                    simplify: 1
                    simplify_at_maxzoom: 0
                    attributes: []
                  areas:
                    geometry: polygon
                    min_size: 4
                    id: true
            "},
            0..=12,
        )
        .expect("lowers")
        .expect("has layers");
        let mut settings = insta::Settings::clone_current();
        settings.add_filter(r"EncoderConfig \{[^}]*\}", "EncoderConfig { .. }");
        let _guard = settings.bind_to_scope();
        insta::assert_debug_snapshot!(scan.def, @r#"
        TableDef {
            columns: [
                Value(
                    "name",
                    Some(
                        String,
                    ),
                ),
                Value(
                    "class",
                    Some(
                        String,
                    ),
                ),
            ],
            layers: [
                LayerDef {
                    name: "roads",
                    output: None,
                    zooms: 4..=12,
                    grid: LayerGrid {
                        extent: 4096,
                        buffer: 0,
                    },
                    clip_mode: Clipped,
                    simplify: PixelThreshold {
                        below_max_zoom: 2.0,
                        at_max_zoom: 2.0,
                    },
                    min_size: PixelThreshold {
                        below_max_zoom: 1.0,
                        at_max_zoom: 0.5,
                    },
                    bounds: None,
                    encoder: EncoderConfig { .. },
                    geometry: [
                        GeometryDef {
                            takes: Some(
                                Line,
                            ),
                            point: None,
                        },
                    ],
                    filter: None,
                    minzoom_expr: None,
                    maxzoom_expr: None,
                    id: Drop,
                    attributes: All,
                    rules: [],
                    sort_by: [],
                    id_order: None,
                    per_tile: [],
                },
                LayerDef {
                    name: "road_points",
                    output: None,
                    zooms: 1..=6,
                    grid: LayerGrid {
                        extent: 1024,
                        buffer: 16,
                    },
                    clip_mode: Unclipped,
                    simplify: PixelThreshold {
                        below_max_zoom: 1.0,
                        at_max_zoom: 0.0,
                    },
                    min_size: PixelThreshold {
                        below_max_zoom: 1.0,
                        at_max_zoom: 0.0625,
                    },
                    bounds: None,
                    encoder: EncoderConfig { .. },
                    geometry: [
                        GeometryDef {
                            takes: Some(
                                Point,
                            ),
                            point: None,
                        },
                    ],
                    filter: None,
                    minzoom_expr: None,
                    maxzoom_expr: None,
                    id: Keep,
                    attributes: None,
                    rules: [],
                    sort_by: [],
                    id_order: None,
                    per_tile: [],
                },
                LayerDef {
                    name: "areas",
                    output: None,
                    zooms: 1..=12,
                    grid: LayerGrid {
                        extent: 1024,
                        buffer: 16,
                    },
                    clip_mode: Unclipped,
                    simplify: PixelThreshold {
                        below_max_zoom: 0.1,
                        at_max_zoom: 0.0625,
                    },
                    min_size: PixelThreshold {
                        below_max_zoom: 4.0,
                        at_max_zoom: 4.0,
                    },
                    bounds: None,
                    encoder: EncoderConfig { .. },
                    geometry: [
                        GeometryDef {
                            takes: Some(
                                Polygon,
                            ),
                            point: None,
                        },
                    ],
                    filter: None,
                    minzoom_expr: None,
                    maxzoom_expr: None,
                    id: Keep,
                    attributes: All,
                    rules: [],
                    sort_by: [],
                    id_order: None,
                    per_tile: [],
                },
            ],
        }
        "#);
    }

    #[test]
    #[expect(clippy::too_many_lines)]
    fn attributes_select_columns_and_jsonb_keys() {
        let scan = lower(
            indoc! {r#"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                properties:
                  class: text
                  name: text
                  "name:de": text
                  "name:en": text
                  rank: int4
                  tags: jsonb
                layers:
                  labels:
                    attributes: [rank, "name:*", name, "name:de"]
                  kinds:
                    attributes: [class, surface]
                  prefixed_keys:
                    attributes: ["surf*"]
            "#},
            0..=14,
        )
        .expect("lowers")
        .expect("has layers");
        let attributes: Vec<_> = scan
            .def
            .layers
            .iter()
            .map(|layer| (layer.name.as_str(), &layer.attributes))
            .collect();
        insta::assert_debug_snapshot!(
            (&scan.def.columns, attributes),
            @r#"
        (
            [
                Value(
                    "class",
                    Some(
                        String,
                    ),
                ),
                Value(
                    "name",
                    Some(
                        String,
                    ),
                ),
                Value(
                    "name:de",
                    Some(
                        String,
                    ),
                ),
                Value(
                    "name:en",
                    Some(
                        String,
                    ),
                ),
                Value(
                    "rank",
                    Some(
                        Int,
                    ),
                ),
                Keys(
                    "tags",
                ),
            ],
            [
                (
                    "labels",
                    Columns(
                        [
                            "rank",
                            "name:de",
                            "name:en",
                            "name",
                        ],
                    ),
                ),
                (
                    "kinds",
                    Columns(
                        [
                            "class",
                            "surface",
                        ],
                    ),
                ),
                (
                    "prefixed_keys",
                    Columns(
                        [],
                    ),
                ),
            ],
        )
        "#
        );
    }

    #[test]
    fn a_layer_outside_the_zooms_is_skipped() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                maxzoom: 10
                layers:
                  roads:
                    maxzoom: 8
                  buildings:
                    minzoom: 11
            "},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        let names: Vec<_> = scan.def.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["roads"]);
    }

    #[test]
    fn a_table_whose_layers_are_all_outside_the_zooms_is_dropped() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                layers:
                  buildings:
                    minzoom: 15
            "},
            0..=14,
        )
        .expect("lowers");
        assert!(scan.is_none());
    }

    #[test]
    fn a_table_outside_the_zooms_is_dropped() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                minzoom: 15
                layers:
                  roads: {}
            "},
            0..=14,
        )
        .expect("lowers");
        assert!(scan.is_none());
    }

    #[test]
    #[expect(clippy::too_many_lines)]
    fn expressions_lower_into_the_layer() {
        let scan = lower(
            indoc! {r#"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                properties:
                  class: text
                  name: text
                  "name:en": text
                  rank: int4
                layers:
                  roads:
                    where: "class != 'service'"
                    minzoom: "rank > 3 ? 4 : 8"
                    maxzoom: 12
                    id: "rank * 10"
                    attributes:
                      kind: class
                      label: { expr: name, minzoom: 10 }
                      source: { value: osm }
                      lanes: 2
                      "name:*": "name:*"
            "#},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        let layer = &scan.def.layers[0];
        insta::assert_debug_snapshot!(
            (&scan.def.columns, &layer.zooms, &layer.filter, &layer.minzoom_expr, &layer.maxzoom_expr, &layer.id, &layer.attributes),
            @r#"
        (
            [
                Value(
                    "class",
                    Some(
                        String,
                    ),
                ),
                Value(
                    "name",
                    Some(
                        String,
                    ),
                ),
                Value(
                    "name:en",
                    Some(
                        String,
                    ),
                ),
                Value(
                    "rank",
                    Some(
                        Int,
                    ),
                ),
            ],
            0..=12,
            Some(
                "class != 'service'",
            ),
            Some(
                "rank > 3 ? 4 : 8",
            ),
            None,
            Expr(
                "rank * 10",
            ),
            Computed {
                attributes: [
                    ComputedAttr {
                        name: "kind",
                        value: Expr(
                            "class",
                        ),
                        zooms: 0..=27,
                    },
                    ComputedAttr {
                        name: "label",
                        value: Expr(
                            "name",
                        ),
                        zooms: 10..=27,
                    },
                    ComputedAttr {
                        name: "source",
                        value: Literal(
                            Str(
                                "osm",
                            ),
                        ),
                        zooms: 0..=27,
                    },
                    ComputedAttr {
                        name: "lanes",
                        value: Literal(
                            I64(
                                2,
                            ),
                        ),
                        zooms: 0..=27,
                    },
                ],
                prefixes: [
                    "name:",
                ],
            },
        )
        "#
        );
    }

    #[test]
    #[expect(clippy::too_many_lines)]
    fn rules_lower_into_the_layer() {
        let scan = lower(
            indoc! {r#"
                schema: public
                table: places
                srid: 4326
                geometry_column: geom
                properties:
                  class: text
                  name: text
                  rank: int4
                  population: int8
                layers:
                  places:
                    attributes:
                      kind: class
                    rules:
                      - where: "class == 'city'"
                        minzoom: 2
                        maxzoom: "population > 1000000 ? 14 : 10"
                        simplify: 2
                        attributes:
                          kind: { value: city }
                          label: { expr: name, minzoom: 6 }
                      - where: "rank > 3"
                        min_size: 4
                      - minzoom: "rank"
            "#},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        insta::assert_debug_snapshot!(scan.def.layers[0].rules, @r#"
        [
            RuleDef {
                when: Some(
                    "class == 'city'",
                ),
                minzoom: Some(
                    Fixed(
                        2,
                    ),
                ),
                maxzoom: Some(
                    Expr(
                        "population > 1000000 ? 14 : 10",
                    ),
                ),
                simplify: Some(
                    PixelThreshold {
                        below_max_zoom: 2.0,
                        at_max_zoom: 2.0,
                    },
                ),
                min_size: None,
                attributes: [
                    ComputedAttr {
                        name: "kind",
                        value: Literal(
                            Str(
                                "city",
                            ),
                        ),
                        zooms: 0..=27,
                    },
                    ComputedAttr {
                        name: "label",
                        value: Expr(
                            "name",
                        ),
                        zooms: 6..=27,
                    },
                ],
                thinning: None,
            },
            RuleDef {
                when: Some(
                    "rank > 3",
                ),
                minzoom: None,
                maxzoom: None,
                simplify: None,
                min_size: Some(
                    PixelThreshold {
                        below_max_zoom: 4.0,
                        at_max_zoom: 4.0,
                    },
                ),
                attributes: [],
                thinning: None,
            },
            RuleDef {
                when: None,
                minzoom: Some(
                    Expr(
                        "rank",
                    ),
                ),
                maxzoom: None,
                simplify: None,
                min_size: None,
                attributes: [],
                thinning: None,
            },
        ]
        "#);
    }

    #[test]
    fn sort_by_lowers_into_the_layer() {
        let scan = lower(
            indoc! {r#"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                properties:
                  class: text
                  name: text
                  rank: int4
                layers:
                  roads:
                    attributes: [name]
                    sort_by: [class, { expr: "rank * 2", desc: true }]
            "#},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        insta::assert_debug_snapshot!(scan.def.layers[0].sort_by, @r#"
        [
            SortDef {
                expr: "class",
                descending: false,
            },
            SortDef {
                expr: "rank * 2",
                descending: true,
            },
        ]
        "#);
    }

    #[test]
    fn a_centroid_geometry_takes_the_features_planetiler_takes_and_draws_their_centroid() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                layers:
                  parks: { geometry: polygon_centroid }
                  roads: { geometry: line_centroid }
                  everything: { geometry: centroid }
            "},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        let geometries: Vec<_> = scan
            .def
            .layers
            .iter()
            .map(|layer| (layer.name.as_str(), &layer.geometry))
            .collect();
        insta::assert_debug_snapshot!(geometries, @r#"
        [
            (
                "parks",
                [
                    GeometryDef {
                        takes: Some(
                            Polygon,
                        ),
                        point: Some(
                            Centroid,
                        ),
                    },
                ],
            ),
            (
                "roads",
                [
                    GeometryDef {
                        takes: Some(
                            Line,
                        ),
                        point: Some(
                            Centroid,
                        ),
                    },
                ],
            ),
            (
                "everything",
                [
                    GeometryDef {
                        takes: None,
                        point: Some(
                            Centroid,
                        ),
                    },
                ],
            ),
        ]
        "#);
    }

    #[test]
    fn a_point_on_surface_geometry_takes_polygons_and_draws_a_point_on_their_surface() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                layers:
                  labels: { geometry: polygon_point_on_surface }
            "},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        let layer = &scan.def.layers[0];
        insta::assert_debug_snapshot!(layer.geometry, @"
        [
            GeometryDef {
                takes: Some(
                    Polygon,
                ),
                point: Some(
                    PointOnSurface,
                ),
            },
        ]
        ");
    }

    #[test]
    fn a_centroid_if_convex_geometry_takes_polygons_and_draws_their_centroid_if_convex() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                layers:
                  labels: { geometry: polygon_centroid_if_convex }
            "},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        let layer = &scan.def.layers[0];
        insta::assert_debug_snapshot!(layer.geometry, @"
        [
            GeometryDef {
                takes: Some(
                    Polygon,
                ),
                point: Some(
                    CentroidIfConvex,
                ),
            },
        ]
        ");
    }

    #[test]
    fn a_geometry_list_lowers_each_entry_in_order() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                layers:
                  addresses: { geometry: [polygon_centroid_if_convex, point] }
            "},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        insta::assert_debug_snapshot!(scan.def.layers[0].geometry, @"
        [
            GeometryDef {
                takes: Some(
                    Polygon,
                ),
                point: Some(
                    CentroidIfConvex,
                ),
            },
            GeometryDef {
                takes: Some(
                    Point,
                ),
                point: None,
            },
        ]
        ");
    }

    #[test]
    fn a_geometry_list_drawing_lines_and_points_is_refused() {
        let err = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                layers:
                  roads: { geometry: [line, polygon_centroid] }
            "},
            0..=14,
        )
        .map(|_| ())
        .expect_err("mixes lines and points");
        insta::assert_snapshot!(err, @"layer `roads`: `geometry: [line, polygon_centroid]` mixes geometry types; the entries of a list must all draw points");
    }

    #[test]
    fn a_geometry_entry_after_entries_taking_all_its_types_is_refused() {
        let err = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                layers:
                  pois: { geometry: [centroid, point] }
            "},
            0..=14,
        )
        .map(|_| ())
        .expect_err("point is never reached");
        insta::assert_snapshot!(err, @"layer `pois`: `point` in `geometry: [centroid, point]` takes no feature, as the entries before it take all of its types");
    }

    #[test]
    #[tracing_test::traced_test]
    fn a_merge_on_a_centroid_layer_warns_that_it_only_has_points() {
        lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                layers:
                  parks:
                    geometry: polygon_centroid
                    merge_polygons: {}
            "},
            0..=14,
        )
        .expect("lowers");
        logs_assert(|lines: &[&str]| {
            let warnings: Vec<_> = lines
                .iter()
                .filter_map(|line| {
                    line.split_once("martin::generate::layers: ")
                        .map(|(_, message)| message)
                })
                .collect();
            insta::assert_snapshot!(warnings.join("\n"), @"`merge_polygons` of layer `parks` in source `roads_source` never acts on its point features");
            Ok(())
        });
    }

    #[test]
    fn unsupported_settings_are_rejected() {
        let layers = [
            "roads: { geometry: line_midpoint }",
            "roads: { geometry: point_on_line }",
            "roads: { geometry: innermost_point }",
        ];
        let errors: Vec<String> = layers
            .iter()
            .map(|layers| {
                let yaml =
                    format!("schema: public\ntable: roads\nsrid: 4326\ngeometry_column: geom\nlayers: {{ {layers} }}");
                lower(&yaml, 0..=14).map(|_| ()).expect_err(layers).to_string()
            })
            .collect();
        insta::assert_snapshot!(errors.join("\n"), @"
        layer `roads`: `geometry: line_midpoint` is not supported by `--mode bulk` yet
        layer `roads`: `geometry: point_on_line` is not supported by `--mode bulk` yet
        layer `roads`: `geometry: innermost_point` is not supported by `--mode bulk` yet
        ");
    }

    #[test]
    fn per_tile_limits_are_clamped_into_the_layer_zooms() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                layers:
                  roads:
                    minzoom: 4
                    limit:
                      - { maxzoom: 2, count: 10 }
                      - { minzoom: 3, maxzoom: 8, count: 50 }
                      - { minzoom: 9, count: 0 }
            "},
            0..=12,
        )
        .expect("lowers")
        .expect("has a layer");
        insta::assert_debug_snapshot!(scan.def.layers[0].per_tile, @"
        [
            TileOpDef {
                zooms: 4..=8,
                op: Limit {
                    count: 50,
                },
            },
            TileOpDef {
                zooms: 9..=12,
                op: Limit {
                    count: 0,
                },
            },
        ]
        ");
    }

    #[test]
    fn grid_thinning_lowers_for_the_layer_and_its_rules() {
        let scan = lower(
            indoc! {"
                schema: public
                table: pois
                srid: 4326
                geometry_column: geom
                buffer: 32
                layers:
                  pois:
                    minzoom: 4
                    thinning:
                      - { maxzoom: 2, method: grid, size: 64, limit: 1 }
                      - { minzoom: 3, maxzoom: 10, method: grid, size: 64, limit: 2, keys: [kind] }
                    rules:
                      - where: \"kind == 'city'\"
                        thinning: { method: grid, size: 1, limit: 1 }
                      - where: \"kind == 'capital'\"
                        thinning: null
                      - minzoom: 6
                  roads:
                    geometry: line
                    thinning: { method: grid, size: 1, limit: 1 }
            "},
            0..=12,
        )
        .expect("lowers")
        .expect("has layers");
        let grids: Vec<_> = scan
            .def
            .layers
            .iter()
            .map(|layer| {
                let rules: Vec<_> = layer.rules.iter().map(|rule| rule.thinning.clone()).collect();
                (layer.name.clone(), layer.per_tile.clone(), rules)
            })
            .collect();
        insta::assert_debug_snapshot!(grids, @r#"
        [
            (
                "pois",
                [
                    TileOpDef {
                        zooms: 4..=10,
                        op: Thinning {
                            method: Grid {
                                size: 64.0,
                                limit: 2,
                            },
                            keys: [
                                "kind",
                            ],
                        },
                    },
                ],
                [
                    Some(
                        [
                            ThinningGrid {
                                zooms: 4..=12,
                                size: 1.0,
                                limit: 1,
                            },
                        ],
                    ),
                    Some(
                        [],
                    ),
                    None,
                ],
            ),
            (
                "roads",
                [
                    TileOpDef {
                        zooms: 0..=12,
                        op: Thinning {
                            method: Grid {
                                size: 1.0,
                                limit: 1,
                            },
                            keys: [],
                        },
                    },
                ],
                [],
            ),
        ]
        "#);
    }

    #[test]
    #[tracing_test::traced_test]
    fn grid_thinning_warns_about_missed_zooms_narrow_buffers_and_layers_without_points() {
        lower(
            indoc! {"
                schema: public
                table: pois
                srid: 4326
                geometry_column: geom
                buffer: 32
                layers:
                  pois:
                    minzoom: 4
                    thinning:
                      - { maxzoom: 2, method: grid, size: 64, limit: 1 }
                      - { minzoom: 3, maxzoom: 10, method: grid, size: 64, limit: 2, keys: [kind] }
                    rules:
                      - where: \"kind == 'city'\"
                        thinning: { method: grid, size: 1, limit: 1 }
                      - where: \"kind == 'capital'\"
                        thinning: null
                      - minzoom: 6
                  roads:
                    geometry: line
                    thinning: { method: grid, size: 1, limit: 1 }
            "},
            0..=12,
        )
        .expect("lowers")
        .expect("has layers");
        logs_assert(|lines: &[&str]| {
            let warnings: Vec<_> = lines
                .iter()
                .filter_map(|line| {
                    line.split_once("martin::generate::layers: ")
                        .map(|(_, message)| message)
                })
                .collect();
            insta::assert_snapshot!(warnings.join("\n"), @"
            Skipping `thinning` of layer `pois` in source `roads_source`: it has no zoom between 4 and 12
            `thinning` of layer `pois` in source `roads_source` has 64 px squares but a 2 px buffer, so neighboring tiles may keep different points of a square crossing their edge; set `buffer` to at least 1024 to avoid it
            `thinning` of layer `roads` in source `roads_source` only thins points by a grid, and the layer has none
            ");
            Ok(())
        });
    }

    #[test]
    #[tracing_test::traced_test]
    fn merge_multi_lowers_with_all_types_by_default() {
        let scan = lower(
            indoc! {"
                schema: public
                table: pois
                srid: 4326
                geometry_column: geom
                layers:
                  pois:
                    geometry: point
                    merge_multi: { geometry: [line], group_by: [kind] }
                  everything:
                    merge_multi: { group_by: [kind] }
            "},
            0..=12,
        )
        .expect("lowers")
        .expect("has layers");
        let ops: Vec<_> = scan
            .def
            .layers
            .iter()
            .map(|layer| (layer.name.clone(), layer.per_tile.clone()))
            .collect();
        insta::assert_debug_snapshot!(ops, @r#"
        [
            (
                "pois",
                [
                    TileOpDef {
                        zooms: 0..=12,
                        op: MergeMulti {
                            geometry: [
                                Line,
                            ],
                            group_by: [
                                "kind",
                            ],
                        },
                    },
                ],
            ),
            (
                "everything",
                [
                    TileOpDef {
                        zooms: 0..=12,
                        op: MergeMulti {
                            geometry: [
                                Point,
                                Line,
                                Polygon,
                            ],
                            group_by: [
                                "kind",
                            ],
                        },
                    },
                ],
            ),
        ]
        "#);
        logs_assert(|lines: &[&str]| {
            let warnings: Vec<_> = lines
                .iter()
                .filter_map(|line| {
                    line.split_once("martin::generate::layers: ")
                        .map(|(_, message)| message)
                })
                .collect();
            insta::assert_snapshot!(warnings.join("\n"), @"`merge_multi` of layer `pois` in source `roads_source` never acts on its point features");
            Ok(())
        });
    }

    #[test]
    fn distance_thinning_lowers_its_keys_and_distance() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                layers:
                  road_labels:
                    thinning: { method: distance, keys: [name, \"ref + '!'\"], distance: 64 }
            "},
            0..=12,
        )
        .expect("lowers")
        .expect("has a layer");
        insta::assert_debug_snapshot!(scan.def.layers[0].per_tile, @r#"
        [
            TileOpDef {
                zooms: 0..=12,
                op: Thinning {
                    method: Distance {
                        distance: 64.0,
                    },
                    keys: [
                        "name",
                        "ref + '!'",
                    ],
                },
            },
        ]
        "#);
    }

    #[test]
    #[tracing_test::traced_test]
    fn merge_lines_clamps_a_buffer_wider_than_the_layers() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                layers:
                  roads:
                    buffer: 64
                    merge_lines: { buffer: 8 }
            "},
            0..=12,
        )
        .expect("lowers")
        .expect("has layers");
        insta::assert_debug_snapshot!(scan.def.layers[0].per_tile, @"
        [
            TileOpDef {
                zooms: 0..=12,
                op: MergeLines {
                    min_length: 0.0,
                    tolerance: 0.0,
                    buffer: Some(
                        4.0,
                    ),
                },
            },
        ]
        ");
        logs_assert(|lines: &[&str]| {
            let warnings: Vec<_> = lines
                .iter()
                .filter_map(|line| {
                    line.split_once("martin::generate::layers: ")
                        .map(|(_, message)| message)
                })
                .collect();
            insta::assert_snapshot!(warnings.join("\n"), @"`merge_lines` of layer `roads` in source `roads_source`: its 8 px buffer is wider than the layer's 4 px, which already clipped the features, so it uses 4 px");
            Ok(())
        });
    }

    #[test]
    #[tracing_test::traced_test]
    fn merge_lines_lowers_per_zoom_range_with_zero_defaults() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                layers:
                  roads:
                    merge_lines:
                      - { maxzoom: 10, min_length: 1, tolerance: 0.5, buffer: 4 }
                      - { minzoom: 11 }
                  areas:
                    geometry: polygon
                    merge_lines: {}
            "},
            0..=12,
        )
        .expect("lowers")
        .expect("has layers");
        insta::assert_debug_snapshot!(scan.def.layers[0].per_tile, @"
        [
            TileOpDef {
                zooms: 0..=10,
                op: MergeLines {
                    min_length: 1.0,
                    tolerance: 0.5,
                    buffer: Some(
                        4.0,
                    ),
                },
            },
            TileOpDef {
                zooms: 11..=12,
                op: MergeLines {
                    min_length: 0.0,
                    tolerance: 0.0,
                    buffer: None,
                },
            },
        ]
        ");
        logs_assert(|lines: &[&str]| {
            let warnings: Vec<_> = lines
                .iter()
                .filter_map(|line| {
                    line.split_once("martin::generate::layers: ")
                        .map(|(_, message)| message)
                })
                .collect();
            insta::assert_snapshot!(warnings.join("\n"), @"`merge_lines` of layer `areas` in source `roads_source` never acts on its polygon features");
            Ok(())
        });
    }

    #[test]
    #[tracing_test::traced_test]
    fn merge_polygons_clamps_a_buffer_wider_than_the_layers() {
        let scan = lower(
            indoc! {"
                schema: public
                table: parks
                srid: 4326
                geometry_column: geom
                layers:
                  parks:
                    buffer: 64
                    merge_polygons: { buffer: 8 }
            "},
            0..=12,
        )
        .expect("lowers")
        .expect("has a layer");
        insta::assert_debug_snapshot!(scan.def.layers[0].per_tile, @"
        [
            TileOpDef {
                zooms: 0..=12,
                op: MergePolygons {
                    min_area: 0.0,
                    min_hole_area: 0.0,
                    distance: 0.0,
                    buffer: Some(
                        4.0,
                    ),
                    group_by: None,
                },
            },
        ]
        ");
        logs_assert(|lines: &[&str]| {
            let warnings: Vec<_> = lines
                .iter()
                .filter_map(|line| {
                    line.split_once("martin::generate::layers: ")
                        .map(|(_, message)| message)
                })
                .collect();
            insta::assert_snapshot!(warnings.join("\n"), @"`merge_polygons` of layer `parks` in source `roads_source`: its 8 px buffer is wider than the layer's 4 px, which already clipped the features, so it uses 4 px");
            Ok(())
        });
    }

    #[test]
    fn merge_polygons_lowers_with_zero_defaults() {
        let scan = lower(
            indoc! {"
                schema: public
                table: landuse
                srid: 4326
                geometry_column: geom
                layers:
                  landuse:
                    merge_polygons: { maxzoom: 12, min_area: 4, distance: 0.5, group_by: [kind] }
            "},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        insta::assert_debug_snapshot!(scan.def.layers[0].per_tile, @r#"
        [
            TileOpDef {
                zooms: 0..=12,
                op: MergePolygons {
                    min_area: 4.0,
                    min_hole_area: 0.0,
                    distance: 0.5,
                    buffer: None,
                    group_by: Some(
                        [
                            "kind",
                        ],
                    ),
                },
            },
        ]
        "#);
    }

    #[test]
    fn layers_of_one_output_layer_share_the_sort_by_and_operations_one_of_them_sets() {
        let scan = lower(
            indoc! {"
                schema: public
                table: parks
                srid: 4326
                geometry_column: geom
                properties:
                  name: text
                  rank: int4
                layers:
                  park:
                    geometry: polygon
                    maxzoom: 10
                    buffer: 4
                    sort_by: [rank]
                  park_label:
                    layer: park
                    geometry: polygon_point_on_surface
                    minzoom: 5
                    buffer: 128
                    limit: { count: 10 }
            "},
            0..=14,
        )
        .expect("lowers")
        .expect("has layers");
        let layers: Vec<_> = scan
            .def
            .layers
            .iter()
            .map(|layer| {
                (
                    &layer.name,
                    &layer.output,
                    &layer.zooms,
                    &layer.sort_by,
                    &layer.per_tile,
                )
            })
            .collect();
        insta::assert_debug_snapshot!(layers, @r#"
        [
            (
                "park",
                None,
                0..=10,
                [
                    SortDef {
                        expr: "rank",
                        descending: false,
                    },
                ],
                [
                    TileOpDef {
                        zooms: 0..=14,
                        op: Limit {
                            count: 10,
                        },
                    },
                ],
            ),
            (
                "park_label",
                Some(
                    "park",
                ),
                5..=14,
                [
                    SortDef {
                        expr: "rank",
                        descending: false,
                    },
                ],
                [
                    TileOpDef {
                        zooms: 0..=14,
                        op: Limit {
                            count: 10,
                        },
                    },
                ],
            ),
        ]
        "#);
    }

    #[test]
    fn layers_of_one_output_layer_must_set_an_operation_equally() {
        let err = lower(
            indoc! {"
                schema: public
                table: parks
                srid: 4326
                geometry_column: geom
                layers:
                  park:
                    limit: { count: 10 }
                  park_label:
                    layer: park
                    limit: { count: 20 }
            "},
            0..=14,
        )
        .expect_err("refused");
        insta::assert_snapshot!(err, @"layers `park` of source `roads_source` and `park_label` of source `roads_source` both write into layer `park` but set `limit` differently; set it equal on both, or on only one of them");
    }
}
