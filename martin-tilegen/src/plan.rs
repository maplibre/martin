//! What to generate: source tables, and the output layers each table feeds.

use std::collections::{BTreeSet, HashSet};
use std::ops::{Range, RangeInclusive};

use martin_core::generate::props::{KeyId, Prop};
use mlt_core::encoder::EncoderConfig;

use crate::expr::{
    Attr, CompiledExpr, ExprError, ExprRef, Filter, Id, Role, SortKey, TypedExpr, Zoom,
};
use crate::{
    FeatureGeom, LayerGrid, LayerInfo, MAX_ZOOM, PixelThreshold, RenderLayer, RenderLayerError,
};

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
/// [`table`]: martin_core::generate::source::FeatureBatch::table
#[derive(Clone, Debug)]
pub struct TableDef {
    /// Property keys known up front, in column order: column `i` is [`KeyId::new(i)`](KeyId::new).
    pub columns: Vec<String>,
    /// Whether features may carry keys beyond `columns`, e.g. from a JSON column.
    pub dynamic_props: bool,
    pub layers: Vec<LayerDef>,
}

/// Expressions are CEL over the feature's properties (see [`expr`](crate::expr)), compiled by [`Plan::new`].
#[derive(Clone, Debug)]
pub struct LayerDef {
    pub name: String,
    pub zooms: RangeInclusive<u8>,
    pub grid: LayerGrid,
    /// RDP tolerance.
    pub simplify: PixelThreshold,
    /// Lines and polygons whose bounding box is smaller than this are dropped.
    pub min_size: PixelThreshold,
    /// WGS84 `[min_lon, min_lat, max_lon, max_lat]`: only tiles intersecting it are generated.
    pub bounds: Option<[f64; 4]>,
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
    /// Unbounded, in source order, keeping ids and all attributes, with
    /// [`PixelThreshold::DEFAULT_SIMPLIFY`] and [`PixelThreshold::DEFAULT_MIN_SIZE`].
    #[must_use]
    pub fn new(name: impl Into<String>, zooms: RangeInclusive<u8>, grid: LayerGrid) -> Self {
        Self {
            name: name.into(),
            zooms,
            grid,
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

    /// The source of every expression of the layer.
    pub fn expressions(&self) -> impl Iterator<Item = &str> {
        let id = match &self.id {
            IdDef::Expr(expr) => Some(expr),
            IdDef::Keep | IdDef::Drop => None,
        };
        let computed = match &self.attributes {
            AttributesDef::Computed { attributes, .. } => attributes.as_slice(),
            AttributesDef::All | AttributesDef::None | AttributesDef::Columns(_) => &[],
        };
        let rules = self.rules.iter().flat_map(|rule| {
            let zooms = [&rule.minzoom, &rule.maxzoom].map(|zoom| match zoom {
                Some(ZoomDef::Expr(expr)) => Some(expr),
                Some(ZoomDef::Fixed(_)) | None => None,
            });
            [rule.when.as_ref()]
                .into_iter()
                .chain(zooms)
                .flatten()
                .chain(value_exprs(&rule.attributes))
        });
        [&self.filter, &self.minzoom_expr, &self.maxzoom_expr]
            .into_iter()
            .flatten()
            .chain(id)
            .chain(value_exprs(computed))
            .chain(rules)
            .chain(self.sort_by.iter().map(|key| &key.expr))
            .map(String::as_str)
    }
}

fn value_exprs(attributes: &[ComputedAttr]) -> impl Iterator<Item = &String> {
    attributes.iter().filter_map(|attr| match &attr.value {
        ValueDef::Expr(expr) => Some(expr),
        ValueDef::Literal(_) => None,
    })
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
    /// These keys, in this column order; a key that is not a table column needs `dynamic_props`.
    Columns(Vec<String>),
    /// `attributes` in this order, then the table columns starting with one of `prefixes`, in column order.
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
#[derive(Debug)]
pub struct Plan {
    pub(crate) tables: Vec<PlannedTable>,
    pub(crate) layers: Vec<PlannedLayer>,
    pub(crate) exprs: Vec<PlannedExpr>,
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
    pub(crate) simplify: PixelThreshold,
    pub(crate) min_size: PixelThreshold,
    pub(crate) geometry: Option<GeometryType>,
    /// Indexes into [`Plan::exprs`].
    pub(crate) filter: Option<ExprRef<Filter>>,
    pub(crate) minzoom: Option<ExprRef<Zoom>>,
    pub(crate) maxzoom: Option<ExprRef<Zoom>>,
    pub(crate) id: PlannedId,
    pub(crate) attributes: AttributesDef,
    /// The output layer's known keys, including those only rules add.
    pub(crate) keys: Vec<String>,
    /// Output key by table column.
    pub(crate) copy: Vec<Option<KeyId>>,
    pub(crate) attrs: PlannedAttrs,
    pub(crate) rules: Vec<PlannedRule>,
    pub(crate) sort_by: Vec<ExprRef<SortKey>>,
}

#[derive(Debug, Default)]
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
pub(crate) struct PlannedRule {
    /// Index into [`Plan::exprs`]; `None` matches every feature.
    pub(crate) when: Option<ExprRef<Filter>>,
    pub(crate) minzoom: Option<PlannedZoom>,
    pub(crate) maxzoom: Option<PlannedZoom>,
    pub(crate) simplify: PixelThreshold,
    pub(crate) min_size: PixelThreshold,
    /// `None` keeps the layer's.
    pub(crate) attrs: Option<PlannedAttrs>,
}

#[derive(Debug)]
pub(crate) enum PlannedZoom {
    Fixed(u8),
    Expr(ExprRef<Zoom>),
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
                    dynamic_props: table.dynamic_props,
                    reads: &mut reads,
                    layer: layers.len(),
                    name: def.name.clone(),
                };
                layers.push(plan_layer(index, def, &mut compiler)?);
            }
            planned_tables.push(PlannedTable {
                reads: reads
                    .into_iter()
                    .filter(|name| !table.columns.contains(name))
                    .collect(),
                columns: table.columns,
                dynamic_props: table.dynamic_props,
                layers: start..layers.len(),
                evaluates: rules || exprs.len() > first_expr,
            });
        }
        Ok(Self {
            tables: planned_tables,
            layers,
            exprs,
        })
    }
}

struct Compiler<'a> {
    exprs: &'a mut Vec<PlannedExpr>,
    columns: &'a [String],
    dynamic_props: bool,
    reads: &'a mut BTreeSet<String>,
    layer: usize,
    name: String,
}

impl Compiler<'_> {
    fn compile<R: Role>(&self, source: &str, role: R) -> Result<TypedExpr<R>, PlanError> {
        TypedExpr::compile(source, self.columns, self.dynamic_props, role).map_err(|error| {
            PlanError::LayerExpr {
                layer: self.name.clone(),
                error,
            }
        })
    }

    fn add<R: Role>(&mut self, source: &str, expr: TypedExpr<R>) -> ExprRef<R> {
        self.reads.extend(expr.columns().map(str::to_owned));
        let (expr, role) = expr.into_parts();
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
            .map(|source| Ok(self.add(source, self.compile(source, role)?)))
            .transpose()
    }
}

fn key_id(pos: usize) -> KeyId {
    KeyId::new(u32::try_from(pos).expect("fewer than 2^32 keys"))
}

fn plan_layer(
    index: u8,
    def: LayerDef,
    compiler: &mut Compiler<'_>,
) -> Result<PlannedLayer, PlanError> {
    let (mut keys, mut copy) = layer_keys(&def, compiler.columns, compiler.dynamic_props)?;
    let (computed, banded) = plan_computed(&def, compiler, &mut copy)?;
    let attrs = PlannedAttrs {
        overrides: Vec::new(),
        bands: zoom_bands(&banded, &def.zooms),
        computed,
        banded,
    };
    let filter = compiler.plan(def.filter.as_ref(), Filter)?;
    let minzoom = compiler.plan(def.minzoom_expr.as_ref(), Zoom::new(&def.zooms))?;
    let maxzoom = compiler.plan(def.maxzoom_expr.as_ref(), Zoom::new(&def.zooms))?;
    let id = match &def.id {
        IdDef::Keep => PlannedId::Keep,
        IdDef::Drop => PlannedId::Drop,
        IdDef::Expr(source) => {
            let expr = compiler.compile(source, Id)?;
            PlannedId::Expr(compiler.add(source, expr))
        }
    };
    let rules = def
        .rules
        .iter()
        .map(|rule| plan_rule(&def, rule, compiler, &mut keys, &attrs))
        .collect::<Result<_, PlanError>>()?;
    let sort_by = def
        .sort_by
        .iter()
        .map(|key| {
            let expr = compiler.compile(
                &key.expr,
                SortKey {
                    descending: key.descending,
                },
            )?;
            Ok(compiler.add(&key.expr, expr))
        })
        .collect::<Result<_, PlanError>>()?;
    let mut render = RenderLayer::new(index, def.zooms.clone(), def.grid).map_err(|error| {
        PlanError::InvalidLayer {
            layer: def.name.clone(),
            error,
        }
    })?;
    render.bounds = def.bounds.map(crate::render::unit_bounds);
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
        simplify: def.simplify,
        min_size: def.min_size,
        geometry: def.geometry,
        filter,
        minzoom,
        maxzoom,
        id,
        attributes: def.attributes,
        keys,
        copy,
        attrs,
        rules,
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

/// A rule's attributes are planned with the layer's they keep, and add their names to `keys`.
fn plan_rule(
    def: &LayerDef,
    rule: &RuleDef,
    compiler: &mut Compiler<'_>,
    keys: &mut Vec<String>,
    layer: &PlannedAttrs,
) -> Result<PlannedRule, PlanError> {
    let when = compiler.plan(rule.when.as_ref(), Filter)?;
    let minzoom = plan_zoom(rule.minzoom.as_ref(), &def.zooms, compiler)?;
    let maxzoom = plan_zoom(rule.maxzoom.as_ref(), &def.zooms, compiler)?;
    let attrs = if rule.attributes.is_empty() {
        None
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
        Some(PlannedAttrs {
            overrides,
            bands: zoom_bands(&layer_banded, &def.zooms),
            computed: layer_computed,
            banded: layer_banded,
        })
    };
    Ok(PlannedRule {
        when,
        minzoom,
        maxzoom,
        simplify: rule.simplify.unwrap_or(def.simplify),
        min_size: rule.min_size.unwrap_or(def.min_size),
        attrs,
    })
}

fn plan_zoom(
    zoom: Option<&ZoomDef>,
    zooms: &RangeInclusive<u8>,
    compiler: &mut Compiler<'_>,
) -> Result<Option<PlannedZoom>, PlanError> {
    Ok(match zoom {
        None => None,
        Some(ZoomDef::Fixed(zoom)) => Some(PlannedZoom::Fixed(
            (*zoom).clamp(*zooms.start(), *zooms.end()),
        )),
        Some(ZoomDef::Expr(source)) => {
            let expr = compiler.compile(source, Zoom::new(zooms))?;
            Some(PlannedZoom::Expr(compiler.add(source, expr)))
        }
    })
}

fn plan_value(value: &ValueDef, compiler: &mut Compiler<'_>) -> Result<PlannedValue, PlanError> {
    Ok(match value {
        ValueDef::Literal(value) => PlannedValue::Literal(value.clone()),
        ValueDef::Expr(source) => {
            let expr = compiler.compile(source, Attr)?;
            let column = expr
                .as_property()
                .and_then(|name| compiler.columns.iter().position(|c| c == name));
            match column {
                Some(column) => PlannedValue::Column(key_id(column)),
                None => PlannedValue::Expr(compiler.add(source, expr)),
            }
        }
    })
}

/// The output layer's known keys, and the output key of each table column copied as it is.
fn layer_keys(
    def: &LayerDef,
    columns: &[String],
    dynamic_props: bool,
) -> Result<(Vec<String>, Vec<Option<KeyId>>), PlanError> {
    let (keys, copied_from) = match &def.attributes {
        AttributesDef::All => (columns.to_vec(), 0),
        AttributesDef::None => (Vec::new(), 0),
        AttributesDef::Columns(keys) => {
            check_duplicates(&def.name, keys)?;
            if !dynamic_props && let Some(key) = keys.iter().find(|key| !columns.contains(key)) {
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
                columns
                    .iter()
                    .filter(|column| prefixes.iter().any(|p| column.starts_with(p.as_str())))
                    .filter(|column| !attributes.iter().any(|attr| attr.name == **column))
                    .cloned(),
            );
            (keys, attributes.len())
        }
    };
    let copy = columns
        .iter()
        .map(|column| {
            keys.iter()
                .skip(copied_from)
                .position(|key| key == column)
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
