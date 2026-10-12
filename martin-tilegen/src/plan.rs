//! What to generate: source tables, and the output layers each table feeds.

use std::collections::{BTreeSet, HashSet};
use std::ops::{Range, RangeInclusive};

use martin_core::generate::props::{KeyId, Prop};
use mlt_core::encoder::EncoderConfig;
use tilejson::Bounds;

use crate::expr::{Attr, CompiledExpr, ExprError, ExprRef, Filter, Id, Role, TypedExpr, Zoom};
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
}

impl LayerDef {
    /// A layer with no [`bounds`](Self::bounds), keeping ids and all attributes in source order, with
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
        [&self.filter, &self.minzoom_expr, &self.maxzoom_expr]
            .into_iter()
            .flatten()
            .chain(id)
            .chain(computed.iter().filter_map(|attr| match &attr.value {
                ValueDef::Expr(expr) => Some(expr),
                ValueDef::Literal(_) => None,
            }))
            .map(String::as_str)
    }
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
    /// The output layer's known keys.
    pub(crate) keys: Vec<String>,
    /// Output key by table column.
    pub(crate) copy: Vec<Option<KeyId>>,
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

#[derive(Debug)]
pub(crate) struct PlannedAttr {
    pub(crate) key: KeyId,
    pub(crate) value: PlannedValue,
    pub(crate) zooms: RangeInclusive<u8>,
}

#[derive(Debug)]
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
            for def in table.layers {
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
                evaluates: exprs.len() > first_expr,
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
    let (keys, mut copy) = layer_keys(&def, compiler.columns, compiler.dynamic_props)?;
    let (computed, banded) = plan_computed(&def, compiler, &mut copy)?;
    let mut bands: Vec<u8> = banded
        .iter()
        .flat_map(|attr| [*attr.zooms.start(), attr.zooms.end().saturating_add(1)])
        .chain([*def.zooms.start()])
        .filter(|zoom| def.zooms.contains(zoom))
        .collect();
    bands.sort_unstable();
    bands.dedup();
    if banded.is_empty() {
        bands.clear();
    }
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
    let render = RenderLayer::new(index, def.zooms.clone(), def.grid).map_err(|error| {
        PlanError::InvalidLayer {
            layer: def.name.clone(),
            error,
        }
    })?;
    let render = match def.bounds {
        Some(bounds) => render.with_bounds(bounds),
        None => render,
    };
    Ok(PlannedLayer {
        render,
        info: LayerInfo {
            name: def.name,
            grid: def.grid,
            encoder: def.encoder,
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
        computed,
        banded,
        bands,
    })
}

/// The output layer's known keys, and the output key of each table column copied as it is.
fn layer_keys(
    def: &LayerDef,
    columns: &[String],
    dynamic_props: bool,
) -> Result<(Vec<String>, Vec<Option<KeyId>>), PlanError> {
    let check_duplicates = |keys: &mut dyn Iterator<Item = &String>| {
        let mut seen = HashSet::new();
        for key in keys {
            if !seen.insert(key) {
                return Err(PlanError::DuplicateAttribute {
                    layer: def.name.clone(),
                    key: key.clone(),
                });
            }
        }
        Ok(())
    };
    let (keys, copied_from) = match &def.attributes {
        AttributesDef::All => (columns.to_vec(), 0),
        AttributesDef::None => (Vec::new(), 0),
        AttributesDef::Columns(keys) => {
            check_duplicates(&mut keys.iter())?;
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
            check_duplicates(&mut attributes.iter().map(|attr| &attr.name))?;
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
        let lo = (*attr.zooms.start()).max(*zooms.start());
        let hi = (*attr.zooms.end()).min(*zooms.end());
        if lo > hi {
            continue;
        }
        let everywhere = lo == *zooms.start() && hi == *zooms.end();
        let value = match &attr.value {
            ValueDef::Literal(value) => PlannedValue::Literal(value.clone()),
            ValueDef::Expr(source) => {
                let expr = compiler.compile(source, Attr)?;
                let column = expr
                    .as_property()
                    .and_then(|name| compiler.columns.iter().position(|c| c == name));
                match column {
                    Some(column) if everywhere && copy[column].is_none() => {
                        copy[column] = Some(key_id(pos));
                        continue;
                    }
                    Some(column) => PlannedValue::Column(key_id(column)),
                    None => PlannedValue::Expr(compiler.add(source, expr)),
                }
            }
        };
        let planned = PlannedAttr {
            key: key_id(pos),
            value,
            zooms: lo..=hi,
        };
        if everywhere {
            computed.push(planned);
        } else {
            banded.push(planned);
        }
    }
    Ok((computed, banded))
}
