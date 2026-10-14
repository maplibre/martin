//! What to generate: source tables, and the output layers each table feeds.

use std::collections::{BTreeSet, HashSet};
use std::ops::{Range, RangeInclusive};

use martin_core::generate::props::{KeyId, Prop};
use mlt_core::encoder::EncoderConfig;
use tilejson::Bounds;

use crate::expr::{Attr, CompiledExpr, ExprError, ExprRef, Filter, Id, Role, Zoom};
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
            let values: Vec<&str> = table
                .columns
                .iter()
                .filter_map(|column| match column {
                    ColumnDef::Value(name) => Some(name.as_str()),
                    ColumnDef::Keys(_) => None,
                })
                .collect();
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
                        || match &column {
                            ColumnDef::Value(name) => reads.contains(name),
                            ColumnDef::Keys(_) => other_keys,
                        };
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
            planned_tables.push(PlannedTable {
                reads: reads
                    .into_iter()
                    .filter(|name| !columns.contains(name))
                    .collect(),
                columns,
                dynamic_props: kept.iter().any(|c| matches!(c, ColumnDef::Keys(_))),
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
        for attr in self.computed.iter_mut().chain(&mut self.banded) {
            if let PlannedValue::Column(key) = &mut attr.value {
                *key = renumbered[key.index()].expect("a column an attribute reads is scanned");
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
            .position(|column| matches!(column, ColumnDef::Value(value) if value == name))
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
    let (keys, mut copy) = layer_keys(&def, compiler)?;
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
    compiler: &Compiler<'_>,
) -> Result<(Vec<String>, Vec<Option<KeyId>>), PlanError> {
    let columns = compiler.columns;
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
        AttributesDef::All => (columns.iter().map(|c| c.name().to_owned()).collect(), 0),
        AttributesDef::None => (Vec::new(), 0),
        AttributesDef::Columns(keys) => {
            check_duplicates(&mut keys.iter())?;
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
            check_duplicates(&mut attributes.iter().map(|attr| &attr.name))?;
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
        let lo = (*attr.zooms.start()).max(*zooms.start());
        let hi = (*attr.zooms.end()).min(*zooms.end());
        if lo > hi {
            continue;
        }
        let everywhere = lo == *zooms.start() && hi == *zooms.end();
        let value = match &attr.value {
            ValueDef::Literal(value) => PlannedValue::Literal(value.clone()),
            ValueDef::Expr(source) => {
                let expr = compiler.compile(source)?;
                let column = expr
                    .as_property()
                    .and_then(|name| compiler.value_position(name));
                match column {
                    Some(column) if everywhere && copy[column].is_none() => {
                        copy[column] = Some(key_id(pos));
                        continue;
                    }
                    Some(column) => PlannedValue::Column(key_id(column)),
                    None => PlannedValue::Expr(compiler.add(source, expr, Attr)),
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
