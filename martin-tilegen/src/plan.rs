//! What to generate: source tables, and the output layers each table feeds.

use std::collections::{BTreeSet, HashSet};
use std::ops::{Range, RangeInclusive};

use crate::expr::CompiledExpr;
use crate::props::{KeyId, Prop};
use crate::{
    FeatureGeom, FeatureOrder, LayerGrid, LayerInfo, MAX_ZOOM, PixelThreshold, RenderLayer,
    TileGenError, TileGenResult,
};

/// A table as its source scans it; its position in [`Plan::new`] is the [`table`] of its batches.
///
/// [`table`]: crate::source::FeatureBatch::table
#[derive(Clone, Debug)]
pub struct TableDef {
    /// Property keys known up front, in column order: column `i` is [`KeyId::from(i)`](KeyId::from).
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
    /// `false` keeps whole features in every tile they touch, like `ST_AsMVTGeom(..., clip_geom => false)`.
    pub clip: bool,
    /// RDP tolerance.
    pub simplify: PixelThreshold,
    /// Lines and polygons whose bounding box is smaller than this are dropped.
    pub min_size: PixelThreshold,
    /// WGS84 `[min_lon, min_lat, max_lon, max_lat]`: only tiles intersecting it are generated.
    pub bounds: Option<[f64; 4]>,
    pub order: FeatureOrder,
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
    /// Clipped, unbounded, in source order, keeping ids and all attributes, with
    /// [`PixelThreshold::PLANETILER_SIMPLIFY`] and [`PixelThreshold::PLANETILER_MIN_SIZE`].
    #[must_use]
    pub fn new(name: impl Into<String>, zooms: RangeInclusive<u8>, grid: LayerGrid) -> Self {
        Self {
            name: name.into(),
            zooms,
            grid,
            clip: true,
            simplify: PixelThreshold::PLANETILER_SIMPLIFY,
            min_size: PixelThreshold::PLANETILER_MIN_SIZE,
            bounds: None,
            order: FeatureOrder::Source,
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
    pub(crate) filter: Option<usize>,
    pub(crate) minzoom: Option<usize>,
    pub(crate) maxzoom: Option<usize>,
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
    Expr(usize),
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
    Expr(usize),
}

impl Plan {
    pub fn new(tables: Vec<TableDef>) -> TileGenResult<Self> {
        let total = tables.iter().map(|t| t.layers.len()).sum();
        if total > 256 {
            return Err(TileGenError::TooManyLayers(total));
        }
        if u16::try_from(tables.len()).is_err() {
            return Err(TileGenError::TooManyTables(tables.len()));
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
                    return Err(TileGenError::DuplicateLayer(def.name));
                }
                if *def.zooms.end() > MAX_ZOOM {
                    return Err(TileGenError::ZoomTooHigh {
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
    fn compile(&self, source: &str) -> TileGenResult<CompiledExpr> {
        CompiledExpr::compile(source, self.columns, self.dynamic_props).map_err(|error| {
            TileGenError::LayerExpr {
                layer: self.name.clone(),
                error,
            }
        })
    }

    fn add(&mut self, source: &str, expr: CompiledExpr) -> usize {
        self.reads.extend(expr.columns().map(str::to_owned));
        self.exprs.push(PlannedExpr {
            expr,
            source: source.to_owned(),
            layer: self.layer,
        });
        self.exprs.len() - 1
    }

    fn plan(&mut self, source: Option<&String>) -> TileGenResult<Option<usize>> {
        source
            .map(|source| Ok(self.add(source, self.compile(source)?)))
            .transpose()
    }
}

fn key_id(pos: usize) -> KeyId {
    KeyId::from(u32::try_from(pos).expect("fewer than 2^32 keys"))
}

fn plan_layer(
    index: u8,
    def: LayerDef,
    compiler: &mut Compiler<'_>,
) -> TileGenResult<PlannedLayer> {
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
    let filter = compiler.plan(def.filter.as_ref())?;
    let minzoom = compiler.plan(def.minzoom_expr.as_ref())?;
    let maxzoom = compiler.plan(def.maxzoom_expr.as_ref())?;
    let id = match &def.id {
        IdDef::Keep => PlannedId::Keep,
        IdDef::Drop => PlannedId::Drop,
        IdDef::Expr(source) => {
            let expr = compiler.compile(source)?;
            PlannedId::Expr(compiler.add(source, expr))
        }
    };
    let mut render = RenderLayer::new(index, def.zooms.clone(), def.grid)?;
    render.clip = def.clip;
    render.bounds = def.bounds.map(crate::render::unit_bounds);
    Ok(PlannedLayer {
        render,
        info: LayerInfo {
            name: def.name,
            grid: def.grid,
            order: def.order,
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
) -> TileGenResult<(Vec<String>, Vec<Option<KeyId>>)> {
    let check_duplicates = |keys: &mut dyn Iterator<Item = &String>| {
        let mut seen = HashSet::new();
        for key in keys {
            if !seen.insert(key) {
                return Err(TileGenError::DuplicateAttribute {
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
                return Err(TileGenError::UnknownColumn {
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
/// a column, at every zoom, is a copy of the column instead, if nothing else copies it.
fn plan_computed(
    def: &LayerDef,
    compiler: &mut Compiler<'_>,
    copy: &mut [Option<KeyId>],
) -> TileGenResult<(Vec<PlannedAttr>, Vec<PlannedAttr>)> {
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
                let renamed = expr
                    .as_property()
                    .and_then(|name| compiler.columns.iter().position(|c| c == name))
                    .filter(|&column| everywhere && copy[column].is_none());
                if let Some(column) = renamed {
                    copy[column] = Some(key_id(pos));
                    continue;
                }
                PlannedValue::Expr(compiler.add(source, expr))
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

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: LayerGrid = LayerGrid {
        extent: 4096,
        buffer: 64,
    };

    #[test]
    fn numbers_layers_table_by_table() {
        let plan = Plan::new(vec![
            TableDef {
                columns: vec!["name".to_owned()],
                dynamic_props: false,
                layers: vec![
                    LayerDef::new("roads", 0..=14, GRID),
                    LayerDef::new("road_labels", 10..=14, GRID),
                ],
            },
            TableDef {
                columns: vec![],
                dynamic_props: false,
                layers: vec![LayerDef::new("water", 0..=14, GRID)],
            },
        ])
        .unwrap();
        let layers: Vec<_> = plan
            .layers
            .iter()
            .map(|l| (l.render.index, l.info.name.as_str()))
            .collect();
        assert_eq!(layers, [(0, "roads"), (1, "road_labels"), (2, "water")]);
        let tables: Vec<_> = plan.tables.iter().map(|t| t.layers.clone()).collect();
        assert_eq!(tables, [0..2, 2..3]);
    }

    #[test]
    fn copies_attribute_columns_in_their_order() {
        let plan = Plan::new(vec![TableDef {
            columns: vec!["name".to_owned(), "class".to_owned(), "lanes".to_owned()],
            dynamic_props: false,
            layers: vec![
                LayerDef::new("all", 0..=14, GRID),
                LayerDef {
                    attributes: AttributesDef::Columns(vec!["lanes".to_owned(), "name".to_owned()]),
                    ..LayerDef::new("subset", 0..=14, GRID)
                },
                LayerDef {
                    attributes: AttributesDef::None,
                    ..LayerDef::new("none", 0..=14, GRID)
                },
            ],
        }])
        .unwrap();
        let copies: Vec<_> = plan.layers.iter().map(|l| l.copy.clone()).collect();
        assert_eq!(
            copies,
            [
                vec![Some(KeyId(0)), Some(KeyId(1)), Some(KeyId(2))],
                vec![Some(KeyId(1)), None, Some(KeyId(0))],
                vec![None, None, None],
            ]
        );
        assert_eq!(plan.layers[1].keys, ["lanes", "name"]);
    }

    #[test]
    fn rejects_a_repeated_layer_name() {
        let err = Plan::new(vec![
            TableDef {
                columns: vec![],
                dynamic_props: false,
                layers: vec![LayerDef::new("roads", 0..=14, GRID)],
            },
            TableDef {
                columns: vec![],
                dynamic_props: false,
                layers: vec![LayerDef::new("roads", 0..=14, GRID)],
            },
        ])
        .unwrap_err();
        assert_eq!(err.to_string(), "layer name `roads` is used more than once");
    }

    #[test]
    fn rejects_more_than_256_layers() {
        let err = Plan::new(vec![TableDef {
            columns: vec![],
            dynamic_props: false,
            layers: (0..257)
                .map(|i| LayerDef::new(format!("l{i}"), 0..=0, GRID))
                .collect(),
        }])
        .unwrap_err();
        assert!(matches!(err, TileGenError::TooManyLayers(257)), "{err}");
    }

    #[test]
    fn rejects_zooms_above_the_max() {
        let err = Plan::new(vec![TableDef {
            columns: vec![],
            dynamic_props: false,
            layers: vec![LayerDef::new(
                "deep",
                0..=28,
                LayerGrid {
                    extent: 1,
                    buffer: 0,
                },
            )],
        }])
        .unwrap_err();
        assert_eq!(err.to_string(), "layer `deep`: zoom 28 is above 27");
    }

    #[test]
    fn rejects_unknown_attribute_columns_without_dynamic_props() {
        let err = Plan::new(vec![TableDef {
            columns: vec!["name".to_owned()],
            dynamic_props: false,
            layers: vec![LayerDef {
                attributes: AttributesDef::Columns(vec!["kind".to_owned()]),
                ..LayerDef::new("roads", 0..=14, GRID)
            }],
        }])
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "layer `roads`: attribute `kind` is not a table column"
        );
        let plan = Plan::new(vec![TableDef {
            columns: vec!["name".to_owned()],
            dynamic_props: true,
            layers: vec![LayerDef {
                attributes: AttributesDef::Columns(vec!["kind".to_owned()]),
                ..LayerDef::new("roads", 0..=14, GRID)
            }],
        }])
        .unwrap();
        assert_eq!(plan.layers[0].copy, [None]);
    }

    #[test]
    fn zoom_bands_start_where_an_attribute_comes_or_goes() {
        let attr = |name: &str, zooms| ComputedAttr {
            name: name.to_owned(),
            value: ValueDef::Expr("rank + 1".to_owned()),
            zooms,
        };
        let plan = Plan::new(vec![TableDef {
            columns: vec!["rank".to_owned()],
            dynamic_props: false,
            layers: vec![
                LayerDef {
                    attributes: AttributesDef::Computed {
                        attributes: vec![attr("a", 0..=30), attr("b", 5..=8), attr("c", 8..=30)],
                        prefixes: vec![],
                    },
                    ..LayerDef::new("banded", 2..=12, GRID)
                },
                LayerDef {
                    attributes: AttributesDef::Computed {
                        attributes: vec![attr("a", 0..=30), attr("never", 13..=14)],
                        prefixes: vec![],
                    },
                    ..LayerDef::new("flat", 2..=12, GRID)
                },
            ],
        }])
        .unwrap();
        let banded = &plan.layers[0];
        assert_eq!(banded.bands, [2, 5, 8, 9]);
        assert_eq!(banded.computed.len(), 1);
        assert_eq!(banded.banded.len(), 2);
        let flat = &plan.layers[1];
        assert_eq!(flat.bands, [] as [u8; 0]);
        assert_eq!(flat.computed.len(), 1);
        assert_eq!(flat.banded.len(), 0);
        assert!(plan.tables[0].evaluates);
    }

    #[test]
    fn an_attribute_that_only_reads_a_column_copies_it() {
        let attr = |name: &str, expr: &str, zooms| ComputedAttr {
            name: name.to_owned(),
            value: ValueDef::Expr(expr.to_owned()),
            zooms,
        };
        let plan = Plan::new(vec![TableDef {
            columns: vec!["class".to_owned(), "name:en".to_owned(), "rank".to_owned()],
            dynamic_props: false,
            layers: vec![LayerDef {
                attributes: AttributesDef::Computed {
                    attributes: vec![
                        attr("kind", "class", 0..=30),
                        attr("label", "feature['name:en']", 0..=30),
                        attr("again", "class", 0..=30),
                        attr("rank", "rank", 9..=30),
                    ],
                    prefixes: vec![],
                },
                ..LayerDef::new("roads", 0..=14, GRID)
            }],
        }])
        .unwrap();
        let layer = &plan.layers[0];
        assert_eq!(layer.copy, [Some(KeyId(0)), Some(KeyId(1)), None]);
        assert_eq!(layer.computed.len(), 1);
        assert_eq!(layer.banded.len(), 1);
        assert_eq!(plan.exprs.len(), 2);
    }

    #[test]
    fn prefixed_columns_follow_the_computed_attributes() {
        let plan = Plan::new(vec![TableDef {
            columns: vec![
                "name:de".to_owned(),
                "name".to_owned(),
                "name:en".to_owned(),
            ],
            dynamic_props: false,
            layers: vec![LayerDef {
                attributes: AttributesDef::Computed {
                    attributes: vec![ComputedAttr {
                        name: "name:en".to_owned(),
                        value: ValueDef::Literal(Prop::Str("fixed".to_owned())),
                        zooms: 0..=30,
                    }],
                    prefixes: vec!["name:".to_owned()],
                },
                ..LayerDef::new("places", 0..=14, GRID)
            }],
        }])
        .unwrap();
        let layer = &plan.layers[0];
        assert_eq!(layer.keys, ["name:en", "name:de"]);
        assert_eq!(layer.copy, [Some(KeyId(1)), None, None]);
        assert!(!plan.tables[0].evaluates);
    }

    #[test]
    fn rejects_an_expression_naming_the_layer() {
        let err = Plan::new(vec![TableDef {
            columns: vec!["class".to_owned()],
            dynamic_props: false,
            layers: vec![LayerDef {
                filter: Some("kind == 'primary'".to_owned()),
                ..LayerDef::new("roads", 0..=14, GRID)
            }],
        }])
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "layer `roads`: `kind == 'primary'` references unknown property `kind`"
        );
    }
}
