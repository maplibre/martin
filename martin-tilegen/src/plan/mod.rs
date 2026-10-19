//! What to generate: source tables, and the output layers each table feeds.

mod eval;
mod filter;

use std::collections::{BTreeSet, HashSet};
use std::ops::{Range, RangeInclusive};
use std::sync::atomic::AtomicBool;

use eval::Keys;
use map_tile_toolkit::ClipMode;
use mlt_core::encoder::EncoderConfig;
use tilejson::Bounds;

use crate::expr::{Attr, CompiledExpr, ExprError, ExprRef, Filter, Id, Role, SortBy, Zoom};
use crate::phase2::LayerInfo;
pub use crate::plan::eval::{Evaluator, ExprErrors, ExprTallies, LayerFeature};
pub use crate::plan::filter::{Condition, RowFilter, TranslateError, Untranslatable};
use crate::props::{KeyId, Prop};
use crate::{FeatureGeom, MAX_ZOOM, PixelThreshold, RenderLayer, RenderLayerError};

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum PlanError {
    #[error("{0} layers exceed the 256 a sort key can address")]
    TooManyLayers(usize),

    #[error("{0} tables exceed the 65536 a feature batch can address")]
    TooManyTables(usize),

    #[error("layer name `{0}` is used more than once")]
    DuplicateLayer(String),

    #[error("layer `{layer}`: zoom {zoom} is above {MAX_ZOOM}")]
    ZoomTooHigh { layer: String, zoom: u8 },

    #[error("layer `{layer}`: attribute `{key}` is listed more than once")]
    DuplicateAttribute { layer: String, key: String },

    #[error("layer `{layer}`: attribute `{column}` is not a table column")]
    UnknownColumn { layer: String, column: String },

    #[error("layer `{layer}`: {error}")]
    LayerExpr { layer: String, error: ExprError },

    #[error("layer `{layer}`: {error}")]
    InvalidLayer {
        layer: String,
        error: RenderLayerError,
    },
}

/// A table as its source scans it; its position in [`Plan::new`] is the [`table`] of its batches.
///
/// [`table`]: crate::source::FeatureBatch::table
#[derive(Clone, Debug)]
pub struct TableDef {
    /// The table's columns. The source scans only those in [`Plan::columns`].
    pub columns: Vec<ColumnDef>,
    pub layers: Vec<LayerDef>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColumnDef {
    /// A property named after the column.
    Value(String),
    /// Properties under keys of their own, e.g. those of a JSON column.
    /// Expressions do not see the column by its name, but a key with that name takes its [`KeyId`].
    Keys(String),
}

impl ColumnDef {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Value(name) | Self::Keys(name) => name,
        }
    }

    /// The property name of a column that is not [`ColumnDef::Keys`].
    fn value(&self) -> Option<&str> {
        match self {
            Self::Value(name) => Some(name),
            Self::Keys(_) => None,
        }
    }
}

/// The tile-local square a fill record stands for, including the buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerGrid {
    pub extent: u32,
    pub buffer: u32,
}

/// Expressions are CEL over the feature's properties (see [`expr`](crate::expr)), compiled by [`Plan::new`].
#[derive(Clone, Debug)]
pub struct LayerDef {
    pub name: String,
    pub zooms: RangeInclusive<u8>,
    pub grid: LayerGrid,
    /// Whether each tile gets only its part of a line or polygon, or the whole geometry.
    pub clip_mode: ClipMode,
    /// RDP tolerance.
    pub simplify: PixelThreshold,
    /// Lines and polygons whose bounding box is smaller than this are dropped.
    pub min_size: PixelThreshold,
    /// WGS84: only tiles intersecting it are generated.
    pub bounds: Option<Bounds>,
    pub encoder: EncoderConfig,
    /// Only features of this type, if set.
    pub geometry: Option<GeometryType>,
    /// Only features for which this is `true`.
    pub filter: Option<String>,
    /// Per feature, clamped into `zooms`; `null` keeps the layer's.
    pub minzoom_expr: Option<String>,
    pub maxzoom_expr: Option<String>,
    pub id: IdDef,
    pub attributes: AttributesDef,
    /// The first rule a feature matches overrides the layer's settings for it.
    pub rules: Vec<RuleDef>,
    /// Orders the layer's features in each tile, ties in source order; empty keeps source order.
    /// Turns off the sort trials of `encoder`.
    pub sort_by: Vec<SortDef>,
}

impl LayerDef {
    /// A clipped layer with no [`bounds`](Self::bounds), keeping ids and all attributes in source
    /// order, with [`PixelThreshold::DEFAULT_SIMPLIFY`] and [`PixelThreshold::DEFAULT_MIN_SIZE`].
    #[must_use]
    pub fn new(name: impl Into<String>, zooms: RangeInclusive<u8>, grid: LayerGrid) -> Self {
        Self {
            name: name.into(),
            zooms,
            grid,
            clip_mode: ClipMode::Clipped,
            simplify: PixelThreshold::DEFAULT_SIMPLIFY,
            min_size: PixelThreshold::DEFAULT_MIN_SIZE,
            bounds: None,
            encoder: EncoderConfig::default(),
            geometry: None,
            filter: None,
            minzoom_expr: None,
            maxzoom_expr: None,
            id: IdDef::Keep,
            attributes: AttributesDef::All,
            rules: Vec::new(),
            sort_by: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RuleDef {
    /// `None` takes every feature, so the rules after it never apply.
    pub when: Option<String>,
    pub minzoom: Option<ZoomDef>,
    pub maxzoom: Option<ZoomDef>,
    pub simplify: Option<PixelThreshold>,
    pub min_size: Option<PixelThreshold>,
    /// These replace the layer's attributes of the same name and add the others; each one replaces the
    /// layer's at every zoom, even outside its own zooms.
    pub attributes: Vec<ComputedAttr>,
}

/// `null`, and a failed evaluation, sort before any value; `descending` reverses that too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortDef {
    pub expr: String,
    pub descending: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZoomDef {
    /// Clamped into the layer's zooms.
    Fixed(u8),
    /// Per feature, clamped into the layer's zooms; `null` keeps the layer's.
    Expr(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryType {
    Point,
    Line,
    Polygon,
}

impl GeometryType {
    pub(crate) fn matches(self, geom: FeatureGeom<'_>) -> bool {
        matches!(
            (self, geom),
            (Self::Point, FeatureGeom::Points(_))
                | (Self::Line, FeatureGeom::Lines(_))
                | (Self::Polygon, FeatureGeom::Polygons(_))
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum IdDef {
    #[default]
    Keep,
    Drop,
    /// A non-negative integer is the id; anything else means none.
    Expr(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum AttributesDef {
    /// Every column and dynamic key.
    #[default]
    All,
    None,
    /// These keys, in this order. a key that is not a [`ColumnDef::Value`] needs a [`ColumnDef::Keys`].
    Columns(Vec<String>),
    /// `attributes` in this order.
    /// Then the [`ColumnDef::Value`]s starting with one of `prefixes`, in column order.
    Computed {
        attributes: Vec<ComputedAttr>,
        prefixes: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComputedAttr {
    pub name: String,
    pub value: ValueDef,
    /// Where the attribute appears; elsewhere the feature goes without it.
    pub zooms: RangeInclusive<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ValueDef {
    Literal(Prop),
    /// `null` leaves the attribute out.
    Expr(String),
}

/// Validated [`TableDef`]s, with output layers numbered table by table, then in declaration order: that
/// number is the layer byte of the sort key, and so the layer's order inside each tile.
///
/// A plan serves one run: it interns the property keys its sources and layers find.
#[derive(Debug)]
pub struct Plan {
    pub(crate) tables: Vec<PlannedTable>,
    pub(crate) layers: Vec<PlannedLayer>,
    pub(crate) exprs: Vec<PlannedExpr>,
    keys: Keys,
    /// Whether an expression has logged its first failure.
    warned: Vec<AtomicBool>,
}

#[derive(Debug)]
pub(crate) struct PlannedTable {
    pub(crate) columns: Vec<String>,
    pub(crate) dynamic_props: bool,
    pub(crate) layers: Range<usize>,
    /// Whether some layer evaluates an expression, so features need a [`FeatureView`](crate::expr::FeatureView).
    pub(crate) evaluates: bool,
    /// Keys beyond `columns` that expressions read by name.
    pub(crate) reads: Vec<String>,
    /// The rows each of its layers takes, in the order of `layers`.
    pub(crate) rows: Vec<Result<Condition, Untranslatable>>,
}

#[derive(Debug)]
pub(crate) struct PlannedExpr {
    pub(crate) expr: CompiledExpr,
    pub(crate) source: String,
    pub(crate) layer: usize,
}

#[derive(Debug)]
pub(crate) struct PlannedLayer {
    pub(crate) render: RenderLayer,
    pub(crate) info: LayerInfo,
    pub(crate) zooms: RangeInclusive<u8>,
    pub(crate) geometry: Option<GeometryType>,
    /// Indexes into [`Plan::exprs`].
    pub(crate) filter: Option<ExprRef<Filter>>,
    pub(crate) id: PlannedId,
    pub(crate) attributes: AttributesDef,
    /// The output layer's known keys, including those only rules add.
    pub(crate) keys: Vec<String>,
    /// Output key by table column.
    pub(crate) copy: Vec<Option<KeyId>>,
    /// The layer's rules, in order.
    pub(crate) cases: Vec<Case>,
    /// The layer's own settings, for features matching no case.
    pub(crate) fallback: Case,
    pub(crate) sort_by: Vec<ExprRef<SortBy>>,
}

/// A rule, with the layer's settings for whatever it does not set.
#[derive(Debug)]
pub(crate) struct Case {
    /// Index into [`Plan::exprs`]; `None` matches every feature.
    pub(crate) when: Option<ExprRef<Filter>>,
    pub(crate) minzoom: PlannedZoom,
    pub(crate) maxzoom: PlannedZoom,
    pub(crate) simplify: PixelThreshold,
    pub(crate) min_size: PixelThreshold,
    pub(crate) attrs: PlannedAttrs,
}

#[derive(Clone, Copy, Debug)]
/// Clamped into the layer's zooms when evaluated.
pub(crate) enum PlannedZoom {
    Fixed(u8),
    /// The first of these that is not `null`; the layer's own zoom if neither.
    Expr(ExprRef<Zoom>, Option<ExprRef<Zoom>>),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PlannedAttrs {
    /// Sorted output keys that copied properties may not take, as these attributes replace them.
    pub(crate) overrides: Vec<KeyId>,
    /// Computed attributes present at every zoom of the layer.
    pub(crate) computed: Vec<PlannedAttr>,
    /// Computed attributes present at some zooms only.
    pub(crate) banded: Vec<PlannedAttr>,
    /// The first zoom of each run of zooms with the same `banded` attributes; empty without any.
    pub(crate) bands: Vec<u8>,
}

#[derive(Debug)]
pub(crate) enum PlannedId {
    Keep,
    Drop,
    Expr(ExprRef<Id>),
}

#[derive(Clone, Debug)]
pub(crate) struct PlannedAttr {
    pub(crate) key: KeyId,
    pub(crate) value: PlannedValue,
    pub(crate) zooms: RangeInclusive<u8>,
}

#[derive(Clone, Debug)]
pub(crate) enum PlannedValue {
    Literal(Prop),
    Column(KeyId),
    Expr(ExprRef<Attr>),
}

impl Plan {
    pub fn new(tables: Vec<TableDef>) -> Result<Self, PlanError> {
        let total = tables.iter().map(|t| t.layers.len()).sum();
        if total > 256 {
            return Err(PlanError::TooManyLayers(total));
        }
        if u16::try_from(tables.len()).is_err() {
            return Err(PlanError::TooManyTables(tables.len()));
        }
        let mut names = HashSet::new();
        let mut planned_tables = Vec::with_capacity(tables.len());
        let mut layers = Vec::with_capacity(total);
        let mut exprs = Vec::new();
        for table in tables {
            let start = layers.len();
            let first_expr = exprs.len();
            let mut reads = BTreeSet::new();
            let values: Vec<&str> = table.columns.iter().filter_map(ColumnDef::value).collect();
            let mut rules = false;
            for def in table.layers {
                rules |= !def.rules.is_empty();
                if !names.insert(def.name.clone()) {
                    return Err(PlanError::DuplicateLayer(def.name));
                }
                if *def.zooms.end() > MAX_ZOOM {
                    return Err(PlanError::ZoomTooHigh {
                        layer: def.name,
                        zoom: *def.zooms.end(),
                    });
                }
                let index = u8::try_from(layers.len()).expect("at most 256 layers");
                let mut compiler = Compiler {
                    exprs: &mut exprs,
                    columns: &table.columns,
                    values: &values,
                    dynamic_props: values.len() < table.columns.len(),
                    reads: &mut reads,
                    layer: layers.len(),
                    name: def.name.clone(),
                };
                layers.push(plan_layer(index, def, &mut compiler)?);
            }
            let table_layers = &mut layers[start..];
            let all_keys = exprs[first_expr..]
                .iter()
                .any(|planned| planned.expr.needs_all_keys());
            let other_keys = all_keys
                || reads.iter().any(|name| !values.contains(&name.as_str()))
                || table_layers.iter().any(|layer| match &layer.attributes {
                    AttributesDef::All => true,
                    AttributesDef::Columns(keys) => {
                        keys.iter().any(|key| !values.contains(&key.as_str()))
                    }
                    AttributesDef::None | AttributesDef::Computed { .. } => false,
                });
            let mut kept = Vec::new();
            let renumbered: Vec<Option<KeyId>> = table
                .columns
                .into_iter()
                .enumerate()
                .map(|(pos, column)| {
                    let scanned = all_keys
                        || table_layers.iter().any(|layer| layer.copy[pos].is_some())
                        || column
                            .value()
                            .map_or(other_keys, |name| reads.contains(name));
                    scanned.then(|| {
                        kept.push(column);
                        key_id(kept.len() - 1)
                    })
                })
                .collect();
            for layer in table_layers {
                layer.renumber(&renumbered);
            }
            let columns: Vec<String> = kept.iter().map(|c| c.name().to_owned()).collect();
            let rows = layers[start..]
                .iter()
                .map(|layer| layer_rows(layer, &exprs))
                .collect();
            planned_tables.push(PlannedTable {
                rows,
                reads: reads
                    .into_iter()
                    .filter(|name| !columns.contains(name))
                    .collect(),
                columns,
                dynamic_props: kept.iter().any(|c| matches!(c, ColumnDef::Keys(_))),
                layers: start..layers.len(),
                evaluates: rules || exprs.len() > first_expr,
            });
        }
        Ok(Self {
            keys: Keys::new(&planned_tables, &layers),
            warned: exprs.iter().map(|_| AtomicBool::new(false)).collect(),
            tables: planned_tables,
            layers,
            exprs,
        })
    }

    /// The columns of table `table` that its layers use, in [`TableDef::columns`] order. The source
    /// scans these, and gives column `i` the key [`KeyId::new(i)`](KeyId::new).
    ///
    /// # Panics
    ///
    /// If the plan has no table `table`.
    #[must_use]
    pub fn columns(&self, table: usize) -> &[String] {
        &self.tables[table].columns
    }

    /// The rows of table `table` that some layer takes at one of `zooms`, for its source to filter on.
    ///
    /// # Panics
    ///
    /// If the plan has no table `table`.
    #[must_use]
    pub fn row_filter(&self, table: usize, zooms: RangeInclusive<u8>) -> RowFilter {
        let table = &self.tables[table];
        let mut conditions = Vec::new();
        for (layer, rows) in self.layers[table.layers.clone()].iter().zip(&table.rows) {
            if layer.zooms.start() > zooms.end() || layer.zooms.end() < zooms.start() {
                continue;
            }
            match rows {
                Ok(condition) => conditions.push(condition.clone()),
                Err(err) => return RowFilter::Untranslatable(err.clone()),
            }
        }
        let condition = Condition::or(conditions);
        if condition == Condition::Const(true) {
            RowFilter::All
        } else {
            RowFilter::Matching(condition)
        }
    }
}

impl PlannedLayer {
    /// From candidate column positions to the scanned ones in `renumbered`.
    fn renumber(&mut self, renumbered: &[Option<KeyId>]) {
        self.copy = self
            .copy
            .iter()
            .zip(renumbered)
            .filter_map(|(copy, column)| column.map(|_| *copy))
            .collect();
        for case in std::iter::once(&mut self.fallback).chain(&mut self.cases) {
            let attrs = &mut case.attrs;
            for attr in attrs.computed.iter_mut().chain(&mut attrs.banded) {
                if let PlannedValue::Column(key) = &mut attr.value {
                    *key = renumbered[key.index()].expect("a column an attribute reads is scanned");
                }
            }
        }
    }
}

struct Compiler<'a> {
    exprs: &'a mut Vec<PlannedExpr>,
    columns: &'a [ColumnDef],
    values: &'a [&'a str],
    dynamic_props: bool,
    reads: &'a mut BTreeSet<String>,
    layer: usize,
    name: String,
}

impl Compiler<'_> {
    fn compile(&mut self, source: &str) -> Result<CompiledExpr, PlanError> {
        let expr =
            CompiledExpr::compile(source, self.values, self.dynamic_props).map_err(|error| {
                PlanError::LayerExpr {
                    layer: self.name.clone(),
                    error,
                }
            })?;
        self.reads.extend(expr.columns().map(str::to_owned));
        Ok(expr)
    }

    fn add<R: Role>(&mut self, source: &str, expr: CompiledExpr, role: R) -> ExprRef<R> {
        self.exprs.push(PlannedExpr {
            expr,
            source: source.to_owned(),
            layer: self.layer,
        });
        ExprRef::new(self.exprs.len() - 1, role)
    }

    fn plan<R: Role>(
        &mut self,
        source: Option<&String>,
        role: R,
    ) -> Result<Option<ExprRef<R>>, PlanError> {
        source
            .map(|source| {
                let expr = self.compile(source)?;
                Ok(self.add(source, expr, role))
            })
            .transpose()
    }

    /// The position of the [`ColumnDef::Value`] named `name`.
    fn value_position(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|column| column.value() == Some(name))
    }
}

/// The rows `layer` takes, by geometry type and `where`.
fn layer_rows(layer: &PlannedLayer, exprs: &[PlannedExpr]) -> Result<Condition, Untranslatable> {
    if let Some(filter) = layer.filter {
        return Err(Untranslatable {
            layer: layer.info.name.clone(),
            expr: exprs[filter.index].source.clone(),
            reason: TranslateError::Where,
        });
    }
    Ok(Condition::and(layer.geometry.map(Condition::Geometry)))
}

fn key_id(pos: usize) -> KeyId {
    KeyId::new(u32::try_from(pos).expect("fewer than 2^32 keys"))
}

fn plan_layer(
    index: u8,
    def: LayerDef,
    compiler: &mut Compiler<'_>,
) -> Result<PlannedLayer, PlanError> {
    let (mut keys, mut copy) = layer_keys(&def, compiler)?;
    let (computed, banded) = plan_computed(&def, compiler, &mut copy)?;
    let attrs = PlannedAttrs {
        overrides: Vec::new(),
        bands: zoom_bands(&banded, &def.zooms),
        computed,
        banded,
    };
    let filter = compiler.plan(def.filter.as_ref(), Filter)?;
    let minzoom = compiler.plan(def.minzoom_expr.as_ref(), Zoom)?;
    let maxzoom = compiler.plan(def.maxzoom_expr.as_ref(), Zoom)?;
    let id = match &def.id {
        IdDef::Keep => PlannedId::Keep,
        IdDef::Drop => PlannedId::Drop,
        IdDef::Expr(source) => {
            let expr = compiler.compile(source)?;
            PlannedId::Expr(compiler.add(source, expr, Id))
        }
    };
    let fallback = Case {
        when: None,
        minzoom: minzoom.map_or(PlannedZoom::Fixed(*def.zooms.start()), |expr| {
            PlannedZoom::Expr(expr, None)
        }),
        maxzoom: maxzoom.map_or(PlannedZoom::Fixed(*def.zooms.end()), |expr| {
            PlannedZoom::Expr(expr, None)
        }),
        simplify: def.simplify,
        min_size: def.min_size,
        attrs,
    };
    let cases = def
        .rules
        .iter()
        .map(|rule| plan_case(&def, rule, compiler, &mut keys, &fallback))
        .collect::<Result<_, PlanError>>()?;
    let sort_by = def
        .sort_by
        .iter()
        .map(|key| {
            let expr = compiler.compile(&key.expr)?;
            let role = SortBy {
                descending: key.descending,
            };
            Ok(compiler.add(&key.expr, expr, role))
        })
        .collect::<Result<_, PlanError>>()?;
    let render = RenderLayer::new(index, def.zooms.clone(), def.grid)
        .map_err(|error| PlanError::InvalidLayer {
            layer: def.name.clone(),
            error,
        })?
        .with_clip_mode(def.clip_mode);
    let render = match def.bounds {
        Some(bounds) => render.with_bounds(bounds),
        None => render,
    };
    Ok(PlannedLayer {
        render,
        info: LayerInfo {
            name: def.name,
            grid: def.grid,
            encoder: if def.sort_by.is_empty() {
                def.encoder
            } else {
                def.encoder
                    .with_id_sort(false)
                    .with_spatial_morton_sort(false)
                    .with_spatial_hilbert_sort(false)
            },
        },
        zooms: def.zooms,
        geometry: def.geometry,
        filter,
        id,
        attributes: def.attributes,
        keys,
        copy,
        cases,
        fallback,
        sort_by,
    })
}

/// The first zoom of each run of zooms with the same `banded` attributes; empty without any.
fn zoom_bands(banded: &[PlannedAttr], zooms: &RangeInclusive<u8>) -> Vec<u8> {
    if banded.is_empty() {
        return Vec::new();
    }
    let mut bands: Vec<u8> = banded
        .iter()
        .flat_map(|attr| [*attr.zooms.start(), attr.zooms.end().saturating_add(1)])
        .chain([*zooms.start()])
        .filter(|zoom| zooms.contains(zoom))
        .collect();
    bands.sort_unstable();
    bands.dedup();
    bands
}

/// The attribute's zooms within the layer's, if any.
fn attr_zooms(attr: &ComputedAttr, zooms: &RangeInclusive<u8>) -> Option<RangeInclusive<u8>> {
    let lo = (*attr.zooms.start()).max(*zooms.start());
    let hi = (*attr.zooms.end()).min(*zooms.end());
    (lo <= hi).then_some(lo..=hi)
}

fn check_duplicates<'a>(
    layer: &str,
    keys: impl IntoIterator<Item = &'a String>,
) -> Result<(), PlanError> {
    let mut seen = HashSet::new();
    for key in keys {
        if !seen.insert(key) {
            return Err(PlanError::DuplicateAttribute {
                layer: layer.to_owned(),
                key: key.clone(),
            });
        }
    }
    Ok(())
}

/// A rule as a case: what it does not set is the layer's, and its attributes add their names to `keys`.
fn plan_case(
    def: &LayerDef,
    rule: &RuleDef,
    compiler: &mut Compiler<'_>,
    keys: &mut Vec<String>,
    fallback: &Case,
) -> Result<Case, PlanError> {
    let when = compiler.plan(rule.when.as_ref(), Filter)?;
    let minzoom = plan_zoom(rule.minzoom.as_ref(), fallback.minzoom, compiler)?;
    let maxzoom = plan_zoom(rule.maxzoom.as_ref(), fallback.maxzoom, compiler)?;
    let layer = &fallback.attrs;
    let attrs = if rule.attributes.is_empty() {
        layer.clone()
    } else {
        check_duplicates(&def.name, rule.attributes.iter().map(|attr| &attr.name))?;
        let (mut overrides, mut computed, mut banded) = (Vec::new(), Vec::new(), Vec::new());
        for attr in &rule.attributes {
            let pos = keys
                .iter()
                .position(|key| *key == attr.name)
                .unwrap_or_else(|| {
                    keys.push(attr.name.clone());
                    keys.len() - 1
                });
            let key = key_id(pos);
            overrides.push(key);
            let Some(zooms) = attr_zooms(attr, &def.zooms) else {
                continue;
            };
            let everywhere = zooms == def.zooms;
            let planned = PlannedAttr {
                key,
                value: plan_value(&attr.value, compiler)?,
                zooms,
            };
            if everywhere {
                computed.push(planned);
            } else {
                banded.push(planned);
            }
        }
        overrides.sort_unstable();
        let kept = |attrs: &[PlannedAttr]| -> Vec<PlannedAttr> {
            attrs
                .iter()
                .filter(|attr| overrides.binary_search(&attr.key).is_err())
                .cloned()
                .collect()
        };
        let (mut layer_computed, mut layer_banded) = (kept(&layer.computed), kept(&layer.banded));
        layer_computed.append(&mut computed);
        layer_banded.append(&mut banded);
        PlannedAttrs {
            overrides,
            bands: zoom_bands(&layer_banded, &def.zooms),
            computed: layer_computed,
            banded: layer_banded,
        }
    };
    Ok(Case {
        when,
        minzoom,
        maxzoom,
        simplify: rule.simplify.unwrap_or(fallback.simplify),
        min_size: rule.min_size.unwrap_or(fallback.min_size),
        attrs,
    })
}

/// A rule's zoom, or the layer's where it sets none or its expression evaluates to `null`.
fn plan_zoom(
    zoom: Option<&ZoomDef>,
    layer: PlannedZoom,
    compiler: &mut Compiler<'_>,
) -> Result<PlannedZoom, PlanError> {
    Ok(match zoom {
        None => layer,
        Some(ZoomDef::Fixed(zoom)) => PlannedZoom::Fixed(*zoom),
        Some(ZoomDef::Expr(source)) => {
            let expr = compiler.compile(source)?;
            let fallback = match layer {
                PlannedZoom::Fixed(_) => None,
                PlannedZoom::Expr(expr, _) => Some(expr),
            };
            PlannedZoom::Expr(compiler.add(source, expr, Zoom), fallback)
        }
    })
}

fn plan_value(value: &ValueDef, compiler: &mut Compiler<'_>) -> Result<PlannedValue, PlanError> {
    Ok(match value {
        ValueDef::Literal(value) => PlannedValue::Literal(value.clone()),
        ValueDef::Expr(source) => {
            let expr = compiler.compile(source)?;
            let column = expr
                .as_property()
                .and_then(|name| compiler.value_position(name));
            match column {
                Some(column) => PlannedValue::Column(key_id(column)),
                None => PlannedValue::Expr(compiler.add(source, expr, Attr)),
            }
        }
    })
}

/// The output layer's known keys, and the output key of each table column copied as it is.
fn layer_keys(
    def: &LayerDef,
    compiler: &Compiler<'_>,
) -> Result<(Vec<String>, Vec<Option<KeyId>>), PlanError> {
    let columns = compiler.columns;
    let (keys, copied_from) = match &def.attributes {
        AttributesDef::All => (columns.iter().map(|c| c.name().to_owned()).collect(), 0),
        AttributesDef::None => (Vec::new(), 0),
        AttributesDef::Columns(keys) => {
            check_duplicates(&def.name, keys)?;
            if !compiler.dynamic_props
                && let Some(key) = keys
                    .iter()
                    .find(|key| !compiler.values.contains(&key.as_str()))
            {
                return Err(PlanError::UnknownColumn {
                    layer: def.name.clone(),
                    column: key.clone(),
                });
            }
            (keys.clone(), 0)
        }
        AttributesDef::Computed {
            attributes,
            prefixes,
        } => {
            check_duplicates(&def.name, attributes.iter().map(|attr| &attr.name))?;
            let mut keys: Vec<String> = attributes.iter().map(|attr| attr.name.clone()).collect();
            keys.extend(
                compiler
                    .values
                    .iter()
                    .filter(|column| prefixes.iter().any(|p| column.starts_with(p.as_str())))
                    .filter(|column| !attributes.iter().any(|attr| attr.name == **column))
                    .map(|column| (*column).to_owned()),
            );
            (keys, attributes.len())
        }
    };
    let copy = columns
        .iter()
        .map(|column| {
            keys.iter()
                .skip(copied_from)
                .position(|key| key == column.name())
                .map(|pos| key_id(copied_from + pos))
        })
        .collect();
    Ok((keys, copy))
}

/// Computed attributes present at every zoom of the layer, and at some zooms only. One that only reads
/// a column takes the column as it is: at every zoom, it is a copy of the column, if nothing else
/// copies it.
fn plan_computed(
    def: &LayerDef,
    compiler: &mut Compiler<'_>,
    copy: &mut [Option<KeyId>],
) -> Result<(Vec<PlannedAttr>, Vec<PlannedAttr>), PlanError> {
    let (mut computed, mut banded) = (Vec::new(), Vec::new());
    let AttributesDef::Computed { attributes, .. } = &def.attributes else {
        return Ok((computed, banded));
    };
    let zooms = &def.zooms;
    for (pos, attr) in attributes.iter().enumerate() {
        let Some(attr_zooms) = attr_zooms(attr, zooms) else {
            continue;
        };
        let everywhere = attr_zooms == *zooms;
        let value = plan_value(&attr.value, compiler)?;
        if let PlannedValue::Column(column) = value
            && everywhere
            && copy[column.index()].is_none()
        {
            copy[column.index()] = Some(key_id(pos));
            continue;
        }
        let planned = PlannedAttr {
            key: key_id(pos),
            value,
            zooms: attr_zooms,
        };
        if everywhere {
            computed.push(planned);
        } else {
            banded.push(planned);
        }
    }
    Ok((computed, banded))
}
