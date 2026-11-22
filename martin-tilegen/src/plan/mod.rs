//! What to generate: source tables, and the output layers each table feeds.

mod branch;
mod eval;
mod filter;

use std::collections::{BTreeSet, HashSet};
use std::num::NonZeroU32;
use std::ops::{Range, RangeInclusive};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use eval::Keys;
use geo::{Centroid as _, Coord, InteriorPoint as _, MultiLineString, MultiPoint, MultiPolygon, Polygon};
use map_tile_toolkit::ClipMode;
use mlt_core::encoder::{EncoderConfig, Network, Profile};
use tilejson::Bounds;

use crate::expr::{Attr, CompiledExpr, ExprError, ExprRef, Filter, Id, Role, SortBy, ThinningKey, Zoom};
use crate::phase1::TILE_PIXELS;
use crate::phase2::{LayerInfo, OpKind};
pub use crate::plan::branch::{Branch, ColumnFacts, Shape, SizeBand};
pub use crate::plan::eval::{Evaluator, ExprErrors, ExprTallies, LayerFeature};
pub use crate::plan::filter::{
    CompareOp, Condition, Literal, Pattern, Polarity, RowFilter, Term, TranslateError, Untranslatable, ValueKind,
};
use crate::props::{KeyId, Prop};
use crate::source::Geometry;
use crate::{FeatureGeom, MAX_ZOOM, PixelThreshold, RenderLayer, RenderLayerError};

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum PlanError {
    #[error("{0} layers exceed the 256 a sort key can address")]
    TooManyLayers(usize),

    #[error("{0} tables exceed the 65536 a feature batch can address")]
    TooManyTables(usize),

    /// Two definitions writing into one output layer that disagree on a setting it has once.
    #[error(
        "layers `{first}` and `{second}` both write into layer `{output}` but with different `{setting}`; it must be \
         equal wherever it is set"
    )]
    OutputMismatch {
        output: String,
        first: String,
        second: String,
        setting: &'static str,
    },

    #[error("layer `{layer}`: zoom {zoom} is above {MAX_ZOOM}")]
    ZoomTooHigh { layer: String, zoom: u8 },

    #[error("layer `{layer}`: attribute `{key}` is listed more than once")]
    DuplicateAttribute { layer: String, key: String },

    #[error("layer `{layer}`: attribute `{column}` is not a table column")]
    UnknownColumn { layer: String, column: String },

    #[error("layer `{layer}`: {error}")]
    LayerExpr { layer: String, error: ExprError },

    #[error("layer `{layer}`: {error}")]
    InvalidLayer { layer: String, error: RenderLayerError },

    #[error("layer `{layer}`: its `geometry` entries draw different geometry types")]
    MixedGeometry { layer: String },

    /// A grid's rank offset so large that a point's rank does not fit an `i64`.
    #[error("layer `{layer}`: a rank of `{attribute}` from `rank_offset` {offset} does not fit a 64-bit integer")]
    RankOverflow {
        layer: String,
        attribute: String,
        offset: i64,
    },

    #[error("layer `{layer}`: `{op}` groups by `{attribute}`, which the layer does not write")]
    UnknownGroupBy {
        layer: String,
        op: &'static str,
        attribute: String,
    },

    /// A hidden attribute the layer does not write.
    #[error("layer `{layer}`: hidden attribute `{attribute}` is not one the layer writes")]
    UnknownHidden { layer: String, attribute: String },

    /// A `thinning` key calling `area`, which only the whole geometry has, not a tile's part of it.
    #[error(
        "layer `{layer}`: thinning key `{expr}` calls `area`, which measures a feature before it is cut into tiles"
    )]
    ThinningArea { layer: String, expr: String },
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
    /// The kind of its values is set when they are all of one kind a source can filter on.
    Value(String, Option<ValueKind>),
    /// Properties under keys of their own, e.g. those of a JSON column.
    /// Expressions do not see the column by its name, but a key with that name takes its [`KeyId`].
    Keys(String),
    /// A JSON document kept whole as one property named after the column, which no source can filter on, since
    /// a source's `null` is not the document's.
    Document(String),
}

impl ColumnDef {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Value(name, _) | Self::Keys(name) | Self::Document(name) => name,
        }
    }

    /// The property name of a column that is not [`ColumnDef::Keys`].
    fn value(&self) -> Option<&str> {
        match self {
            Self::Value(name, _) | Self::Document(name) => Some(name),
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
    /// Names the definition in errors and paths, and the output layer unless [`Self::output`] does.
    pub name: String,
    /// The output layer it writes into, which other definitions may share; [`Self::name`] when `None`.
    ///
    /// Definitions sharing one are merged in each tile, each keeping its own features, zooms, grid buffer,
    /// simplification, attributes and rules. They must have equal extents and encoders, and equal `sort_by`
    /// and operations of each kind in `per_tile` wherever those are not empty; the output layer uses them.
    pub output: Option<String>,
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
    /// The features the layer takes and what it draws for each: a feature is drawn by the first entry that takes
    /// its type, and left out when none does. Empty takes every feature as it is.
    pub geometry: Vec<GeometryDef>,
    /// Only features for which this is `true`.
    pub filter: Option<String>,
    /// Per feature, clamped into `zooms`; `null` keeps the layer's.
    pub minzoom_expr: Option<String>,
    pub maxzoom_expr: Option<String>,
    pub id: IdDef,
    pub attributes: AttributesDef,
    /// The first rule a feature matches overrides the layer's settings for it.
    pub rules: Vec<RuleDef>,
    /// Orders the output layer's features in each tile, ties in definition order, then source order; empty keeps
    /// that order. Turns off the sort trials of `encoder`.
    pub sort_by: Vec<SortDef>,
    /// Orders the output layer's features that tie on `sort_by` by their source id, those without one last. Turns off
    /// the sort trials of `encoder`.
    pub id_order: Option<IdOrder>,
    /// Run on the output layer's features in each tile after `sort_by`, in [`TileOp`] order whatever their order
    /// here, clamped into the zooms of all its definitions. Zoom ranges of one kind of operation must not overlap.
    pub per_tile: Vec<TileOpDef>,
    /// Output attributes that `per_tile` operations read but tiles leave out.
    pub hidden: Vec<String>,
}

impl LayerDef {
    /// A clipped layer with no [`bounds`](Self::bounds), keeping ids and all attributes in source
    /// order, with [`PixelThreshold::DEFAULT_SIMPLIFY`] and [`PixelThreshold::DEFAULT_MIN_SIZE`].
    #[must_use]
    pub fn new(name: impl Into<String>, zooms: RangeInclusive<u8>, grid: LayerGrid) -> Self {
        Self {
            name: name.into(),
            output: None,
            zooms,
            grid,
            clip_mode: ClipMode::Clipped,
            simplify: PixelThreshold::DEFAULT_SIMPLIFY,
            min_size: PixelThreshold::DEFAULT_MIN_SIZE,
            bounds: None,
            encoder: EncoderConfig::from_profile(Profile::Web(Network::Offline)),
            geometry: Vec::new(),
            filter: None,
            minzoom_expr: None,
            maxzoom_expr: None,
            id: IdDef::Keep,
            attributes: AttributesDef::All,
            rules: Vec::new(),
            sort_by: Vec::new(),
            id_order: None,
            per_tile: Vec::new(),
            hidden: Vec::new(),
        }
    }
}

/// Planetiler's point label grid: at the zooms it covers, the world is cut into squares of `size` pixels
/// of a 256-pixel tile, and each tile keeps only the first `limit` points of each square, in draw order.
/// Squares smaller than one unit of the layer's extent do not limit anything.
#[derive(Clone, Debug, PartialEq)]
pub struct ThinningGrid {
    /// Clamped into the layer's zooms.
    pub zooms: RangeInclusive<u8>,
    /// Side of a square, in pixels of a 256-pixel tile.
    pub size: f64,
    /// Points kept per square and tile.
    pub limit: NonZeroU32,
}

/// A [`TileOp`] and the zooms it runs at; outside them the layer's features pass through it unchanged.
#[derive(Clone, Debug, PartialEq)]
pub struct TileOpDef {
    /// Clamped into the layer's zooms; an operation with none of them is dropped.
    pub zooms: RangeInclusive<u8>,
    pub op: TileOp,
}

/// An operation on all of one layer's features in one tile. Variants are declared in the order they run.
#[derive(Clone, Debug, PartialEq)]
pub enum TileOp {
    /// Joins the lines of a group whose ends meet, where exactly two ends do, then simplifies them by
    /// `tolerance`, clips them to the tile and `buffer`, and drops those shorter than `min_length`. Each
    /// group becomes one line or multi-line feature, or none.
    MergeLines {
        /// Pixels of a 256-pixel tile.
        min_length: f64,
        /// Pixels of a 256-pixel tile; 0 keeps every vertex.
        tolerance: f64,
        /// Pixels of a 256-pixel tile around the tile; the layer's buffer if `None`.
        buffer: Option<f64>,
    },
    /// Unions the polygons of a group, clipped to the tile and `buffer`: joined across gaps narrower than
    /// `distance`, holes smaller than `min_hole_area` filled, polygons smaller than `min_area` dropped.
    /// Each group becomes one polygon or multi-polygon feature, or none.
    MergePolygons {
        /// Square pixels of a 256-pixel tile.
        min_area: f64,
        /// Square pixels of a 256-pixel tile.
        min_hole_area: f64,
        /// Pixels of a 256-pixel tile; 0 joins only touching or overlapping polygons.
        distance: f64,
        /// Pixels of a 256-pixel tile around the tile; the layer's buffer if `None`.
        buffer: Option<f64>,
        /// Output attributes that must be equal, a missing one counting as `null`; all of them if `None`.
        group_by: Option<Vec<String>>,
    },
    /// Drops features that an earlier kept feature with the same key crowds out, as `method` says. The key
    /// is the values of `keys` over the feature's output attributes; without `keys` every feature has the
    /// same key. A key of only `null`s is never thinned.
    Thinning {
        method: ThinningMethod,
        /// CEL expressions over the feature's output attributes, by output name.
        keys: Vec<String>,
    },
    /// Combines the features of a group and type into one multi-geometry, with the parts in draw order,
    /// and only the `group_by` attributes.
    MergeMulti {
        /// The types to combine; others pass through.
        geometry: Vec<GeometryType>,
        /// Output attributes that must be equal, a missing one counting as `null`, and that the merged
        /// feature keeps.
        group_by: Vec<String>,
    },
    /// Keeps the first `count` features and drops the rest.
    Limit { count: u32 },
}

impl TileOp {
    /// Position in the fixed order operations run in.
    fn rank(&self) -> u8 {
        match self {
            Self::MergeLines { .. } => 1,
            Self::MergePolygons { .. } => 2,
            Self::Thinning { .. } => 3,
            Self::MergeMulti { .. } => 4,
            Self::Limit { .. } => 5,
        }
    }

    /// The names of the operations, in the order they run.
    const KEYS: [&str; 5] = ["merge_lines", "merge_polygons", "thinning", "merge_multi", "limit"];

    /// The operation's name as configurations write it.
    const fn key(&self) -> &'static str {
        match self {
            Self::MergeLines { .. } => "merge_lines",
            Self::MergePolygons { .. } => "merge_polygons",
            Self::Thinning { .. } => "thinning",
            Self::MergeMulti { .. } => "merge_multi",
            Self::Limit { .. } => "limit",
        }
    }
}

/// How [`TileOp::Thinning`] decides that an earlier kept feature crowds a feature out.
#[derive(Clone, Debug, PartialEq)]
pub enum ThinningMethod {
    /// Keeps the first points of each square of a world grid; see [`ThinningGrid`]. Lines and polygons
    /// pass through.
    Grid {
        /// Side of a square, in pixels of a 256-pixel tile.
        size: f64,
        /// Points kept per square and key.
        limit: NonZeroU32,
        /// Writes each kept point's position in its square to an attribute.
        rank: Option<GridRank>,
    },
    /// Drops a feature within `distance` pixels of a 256-pixel tile of an earlier kept one with its key,
    /// measured between representative points.
    Distance { distance: f64 },
    /// Drops a feature whose key an earlier kept one has anywhere in the tile.
    Tile,
}

/// Planetiler's `gridrank`: each kept point of a grid square gets its 1-based position among the kept points
/// of its square and key, in draw order, plus `offset`, unless it already has the attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridRank {
    /// The output attribute written.
    pub attribute: String,
    /// Added to the position.
    pub offset: i64,
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
    /// Replaces the layer's grid thinning for the features this rule takes; empty turns it off for them.
    pub thinning: Option<Vec<ThinningGrid>>,
}

/// `null`, and a failed evaluation, sort before any value; `descending` reverses that too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortDef {
    pub expr: String,
    pub descending: bool,
}

/// The direction [`LayerDef::id_order`] orders source ids in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdOrder {
    /// Smallest id first.
    Ascending,
    /// Largest id first.
    Descending,
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

/// One entry of a layer's `geometry`: the features it takes, and what it draws for each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeometryDef {
    /// Only features of this type, if set.
    pub takes: Option<GeometryType>,
    /// Each feature it takes becomes this point of its geometry, if set.
    pub point: Option<DerivedPoint>,
}

impl GeometryDef {
    /// Every feature, as it is.
    pub const ANY: Self = Self {
        takes: None,
        point: None,
    };

    /// The type of the geometry it draws, if it has only one.
    #[must_use]
    pub fn draws(self) -> Option<GeometryType> {
        self.point.map_or(self.takes, |_| Some(GeometryType::Point))
    }

    /// Whether it takes a feature with `geometry`, projected as `geom`: one of its type, or any but an
    /// [`Geometry::Outline`], whose polygon it takes instead.
    pub(crate) fn accepts(self, geometry: &Geometry, geom: FeatureGeom<'_>) -> bool {
        match self.takes {
            Some(kind) => kind.matches(geom),
            None => !matches!(geometry, Geometry::Outline(_)),
        }
    }
}

/// Features of the type, as they are.
impl From<GeometryType> for GeometryDef {
    fn from(kind: GeometryType) -> Self {
        Self {
            takes: Some(kind),
            point: None,
        }
    }
}

/// A point computed from a feature's projected geometry, which the layer draws in its place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DerivedPoint {
    /// The centroid of the highest dimension the geometry has: area-weighted for polygons,
    /// length-weighted for lines, the mean for points.
    Centroid,
    /// A point on the geometry, as geo's `InteriorPoint` finds it over all parts of a multi-geometry.
    PointOnSurface,
    /// As Planetiler's `centroidIfConvex`: the centroid of a convex polygon without holes, a point on the surface
    /// of any other polygon or multi-polygon, and the centroid of lines and points.
    CentroidIfConvex,
}

impl DerivedPoint {
    /// The point of a feature with `geometry`, projected as `geom`, or `None` when it has none, such as an empty
    /// geometry; that of the polygon of an [`Geometry::Outline`], as Planetiler derives it from a closed way that is
    /// also an area, and a point on the surface of a multi-polygon of one part for [`Self::CentroidIfConvex`].
    pub(crate) fn of(self, geometry: &Geometry, geom: FeatureGeom<'_>) -> Option<Coord<f64>> {
        let area;
        let geom = if let Geometry::Outline(ring) = geometry {
            area = Polygon::new(ring.clone(), Vec::new());
            FeatureGeom::Polygons(std::slice::from_ref(&area))
        } else {
            geom
        };
        if matches!((self, geometry), (Self::CentroidIfConvex, Geometry::Polygons(_))) {
            return Self::PointOnSurface.of_geom(geom);
        }
        self.of_geom(geom)
    }

    /// The point of `geom`, or `None` when it has none.
    fn of_geom(self, geom: FeatureGeom<'_>) -> Option<Coord<f64>> {
        let point = match (self, geom) {
            (Self::CentroidIfConvex, _) => {
                let centroid = match geom {
                    FeatureGeom::Polygons([polygon]) => {
                        polygon.interiors().is_empty() && is_convex(&polygon.exterior().0)
                    }
                    FeatureGeom::Polygons(_) => false,
                    FeatureGeom::Points(_) | FeatureGeom::Lines(_) => true,
                };
                return if centroid { Self::Centroid } else { Self::PointOnSurface }.of_geom(geom);
            }
            (Self::Centroid | Self::PointOnSurface, FeatureGeom::Points([point])) => return Some(*point),
            (Self::Centroid, FeatureGeom::Points(points)) => MultiPoint::from(points.to_vec()).centroid(),
            (Self::Centroid, FeatureGeom::Lines([line])) => line.centroid(),
            (Self::Centroid, FeatureGeom::Lines(lines)) => MultiLineString::new(lines.to_vec()).centroid(),
            (Self::Centroid, FeatureGeom::Polygons([polygon])) => polygon.centroid(),
            (Self::Centroid, FeatureGeom::Polygons(polygons)) => MultiPolygon::new(polygons.to_vec()).centroid(),
            (Self::PointOnSurface, FeatureGeom::Points(points)) => MultiPoint::from(points.to_vec()).interior_point(),
            (Self::PointOnSurface, FeatureGeom::Lines([line])) => line.interior_point(),
            (Self::PointOnSurface, FeatureGeom::Lines(lines)) => MultiLineString::new(lines.to_vec()).interior_point(),
            (Self::PointOnSurface, FeatureGeom::Polygons([polygon])) => polygon.interior_point(),
            (Self::PointOnSurface, FeatureGeom::Polygons(polygons)) => {
                MultiPolygon::new(polygons.to_vec()).interior_point()
            }
        };
        point.map(|point| point.0)
    }
}

/// Whether `ring` is convex as Planetiler's `GeoUtils.isConvex` decides it: the triangles of consecutive points keep
/// one orientation, ignoring repeated points and turns below a thousandth of the largest one the other way. Unlike
/// Planetiler, a repeated point keeps the previous edge, so a repeated corner still turns.
fn is_convex(ring: &[Coord<f64>]) -> bool {
    const THRESHOLD: f64 = 1e-3;
    const MIN_POINTS_TO_CHECK: usize = 10;
    let ratio = |neg: f64, pos: f64| if neg < pos { neg / pos } else { pos / neg };
    let size = ring.len();
    if size <= 3 {
        return false;
    }
    let c0 = ring[0];
    let mut i = 1;
    let mut c1 = ring[i];
    while i < size {
        c1 = ring[i];
        if c1 != c0 {
            break;
        }
        i += 1;
    }
    let mut d1 = c1 - c0;
    let (mut neg, mut pos) = (1e-20, 1e-20);
    while i <= size + 1 {
        let c2 = ring[if i < size { i } else { i + 1 - size }];
        let d2 = c2 - c1;
        let z = d1.x * d2.y - d1.y * d2.x;
        let mut extended = false;
        if z < 0.0 && -z > neg {
            neg = -z;
            extended = true;
        } else if z > 0.0 && z > pos {
            pos = z;
            extended = true;
        }
        if (i == MIN_POINTS_TO_CHECK || (i > MIN_POINTS_TO_CHECK && extended)) && ratio(neg, pos) > THRESHOLD {
            return false;
        }
        c1 = c2;
        if d2 != Coord::zero() {
            d1 = d2;
        }
        i += 1;
    }
    ratio(neg, pos) < THRESHOLD
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

/// Validated [`TableDef`]s, with layer definitions numbered table by table, then in declaration order: that
/// number is the layer byte of the sort key. Output layers are numbered in the order their first definition comes
/// in, which is their order inside each tile.
///
/// A plan serves one run: it interns the property keys its sources and layers find.
#[derive(Debug)]
pub struct Plan {
    pub(crate) tables: Vec<PlannedTable>,
    pub(crate) layers: Vec<PlannedLayer>,
    pub(crate) outputs: Vec<PlannedOutput>,
    pub(crate) exprs: Vec<PlannedExpr>,
    keys: Keys,
    /// Whether an expression has logged its first failure.
    warned: Vec<AtomicBool>,
}

/// An output layer: the definitions writing into it, and what it has once for all of them.
#[derive(Debug)]
pub(crate) struct PlannedOutput {
    pub(crate) info: LayerInfo,
    /// Indexes into [`Plan::layers`] of its definitions, in plan order.
    pub(crate) layers: Vec<usize>,
    /// Its known keys: those of each of its definitions in turn, including those only rules add.
    pub(crate) keys: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct PlannedTable {
    pub(crate) columns: Vec<String>,
    /// The scanned columns, as [`Self::columns`] names them.
    pub(crate) defs: Vec<ColumnDef>,
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

/// A layer definition, as it takes features and renders them for its output layer.
#[derive(Debug)]
pub(crate) struct PlannedLayer {
    /// The definition's name.
    pub(crate) name: String,
    /// Index into [`Plan::outputs`] of the output layer it writes into.
    pub(crate) output: usize,
    pub(crate) render: RenderLayer,
    pub(crate) zooms: RangeInclusive<u8>,
    /// The entries of [`LayerDef::geometry`], never empty.
    pub(crate) geometry: Vec<GeometryDef>,
    /// Indexes into [`Plan::exprs`].
    pub(crate) filter: Option<ExprRef<Filter>>,
    pub(crate) id: PlannedId,
    pub(crate) attributes: AttributesDef,
    /// Output key by table column.
    pub(crate) copy: Vec<Option<KeyId>>,
    /// The layer's rules, in order.
    pub(crate) cases: Vec<Case>,
    /// The layer's own settings, for features matching no case.
    pub(crate) fallback: Case,
    pub(crate) sort_by: Vec<ExprRef<SortBy>>,
    pub(crate) id_order: Option<IdOrder>,
}

impl PlannedLayer {
    /// The type of the geometry the layer draws, if it has only one.
    pub(crate) fn output_geometry(&self) -> Option<GeometryType> {
        self.geometry[0].draws()
    }

    /// The entry of the layer's `geometry` that draws a feature with `geometry`, projected as `geom`, or `None` when
    /// the layer leaves it out. A layer with an entry taking polygons leaves out every [`Geometry::Outline`], as it
    /// draws the closed way from its polygon.
    pub(crate) fn entry(&self, geometry: &Geometry, geom: FeatureGeom<'_>) -> Option<GeometryDef> {
        if matches!(geometry, Geometry::Outline(_))
            && self
                .geometry
                .iter()
                .any(|entry| entry.takes.is_none_or(|kind| kind == GeometryType::Polygon))
        {
            return None;
        }
        self.geometry
            .iter()
            .copied()
            .find(|entry| entry.accepts(geometry, geom))
    }

    /// The geometry types the layer takes, or `None` when it takes all.
    pub(crate) fn geometry_condition(&self) -> Option<Condition> {
        let mut kinds = Vec::new();
        for entry in &self.geometry {
            let kind = entry.takes?;
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
        Some(Condition::or(kinds.into_iter().map(Condition::Geometry)))
    }

    /// Whether some of the layer's features are thinned by a grid.
    pub(crate) fn has_thinning_grid(&self) -> bool {
        std::iter::once(&self.fallback)
            .chain(&self.cases)
            .any(|case| !case.thinning_grid.is_empty())
    }
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
    /// Clamped into the layer's zooms.
    pub(crate) thinning_grid: Vec<ThinningGrid>,
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
        let drafts = OutputDraft::all(&tables)?;
        let mut planned_tables = Vec::with_capacity(tables.len());
        let mut layers = Vec::with_capacity(total);
        let mut own_keys = Vec::with_capacity(total);
        let mut exprs = Vec::new();
        for table in tables {
            let start = layers.len();
            let first_expr = exprs.len();
            let mut reads = BTreeSet::new();
            let values: Vec<&str> = table.columns.iter().filter_map(ColumnDef::value).collect();
            let mut rules = false;
            for def in table.layers {
                rules |= !def.rules.is_empty();
                if *def.zooms.end() > MAX_ZOOM {
                    return Err(PlanError::ZoomTooHigh {
                        layer: def.name,
                        zoom: *def.zooms.end(),
                    });
                }
                let index = u8::try_from(layers.len()).expect("at most 256 layers");
                let output = drafts
                    .iter()
                    .position(|draft| draft.layers.contains(&layers.len()))
                    .expect("every definition writes into an output layer");
                let mut compiler = Compiler {
                    exprs: &mut exprs,
                    columns: &table.columns,
                    values: &values,
                    dynamic_props: values.len() < table.columns.len(),
                    reads: &mut reads,
                    layer: layers.len(),
                    name: def.name.clone(),
                };
                let (layer, keys) = plan_layer(index, def, output, &drafts[output], &mut compiler)?;
                own_keys.push((keys, compiler.dynamic_props && layer.attributes == AttributesDef::All));
                layers.push(layer);
            }
            let table_layers = &mut layers[start..];
            let all_keys = exprs[first_expr..].iter().any(|planned| planned.expr.needs_all_keys());
            let other_keys = all_keys
                || reads.iter().any(|name| !values.contains(&name.as_str()))
                || table_layers.iter().any(|layer| match &layer.attributes {
                    AttributesDef::All => true,
                    AttributesDef::Columns(keys) => keys.iter().any(|key| !values.contains(&key.as_str())),
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
                        || column.value().map_or(other_keys, |name| reads.contains(name));
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
                .map(|layer| layer_rows(&kept, layer, &exprs))
                .collect();
            planned_tables.push(PlannedTable {
                rows,
                reads: reads.into_iter().filter(|name| !columns.contains(name)).collect(),
                columns,
                dynamic_props: kept.iter().any(|c| matches!(c, ColumnDef::Keys(_))),
                defs: kept,
                layers: start..layers.len(),
                evaluates: rules || exprs.len() > first_expr,
            });
        }
        let outputs = drafts
            .into_iter()
            .map(|draft| draft.plan(&mut layers, &own_keys, &mut exprs))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            keys: Keys::new(&planned_tables, &outputs),
            warned: exprs.iter().map(|_| AtomicBool::new(false)).collect(),
            tables: planned_tables,
            layers,
            outputs,
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
        self.layers_row_filter(table, zooms, |_| true)
    }

    /// The rows of table `table` that some layer of only `geometry` takes at one of `zooms`, leaving out the layers
    /// of any geometry, for its source to filter readings on that only such layers draw.
    ///
    /// # Panics
    ///
    /// If the plan has no table `table`.
    #[must_use]
    pub fn typed_row_filter(&self, table: usize, zooms: RangeInclusive<u8>, geometry: GeometryType) -> RowFilter {
        self.layers_row_filter(table, zooms, |layer| {
            layer.geometry.iter().any(|entry| entry.takes == Some(geometry))
        })
    }

    /// The rows of table `table` that some layer for which `of` holds takes at one of `zooms`.
    fn layers_row_filter(
        &self,
        table: usize,
        zooms: RangeInclusive<u8>,
        of: impl Fn(&PlannedLayer) -> bool,
    ) -> RowFilter {
        let table = &self.tables[table];
        let mut conditions = Vec::new();
        for (layer, rows) in self.layers[table.layers.clone()].iter().zip(&table.rows) {
            if !of(layer) || layer.zooms.start() > zooms.end() || layer.zooms.end() < zooms.start() {
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
    /// From the definition's own output keys to those of its output layer, `keys[i]` being that of its key `i`.
    fn rekey(&mut self, keys: &[KeyId]) {
        for key in self.copy.iter_mut().flatten() {
            *key = keys[key.index()];
        }
        for case in std::iter::once(&mut self.fallback).chain(&mut self.cases) {
            let attrs = &mut case.attrs;
            for key in &mut attrs.overrides {
                *key = keys[key.index()];
            }
            attrs.overrides.sort_unstable();
            for attr in attrs.computed.iter_mut().chain(&mut attrs.banded) {
                attr.key = keys[attr.key.index()];
            }
        }
    }

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
            CompiledExpr::compile(source, self.values, self.dynamic_props).map_err(|error| PlanError::LayerExpr {
                layer: self.name.clone(),
                error,
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

    fn plan<R: Role>(&mut self, source: Option<&String>, role: R) -> Result<Option<ExprRef<R>>, PlanError> {
        source
            .map(|source| {
                let expr = self.compile(source)?;
                Ok(self.add(source, expr, role))
            })
            .transpose()
    }

    /// The position of the [`ColumnDef::Value`] or [`ColumnDef::Document`] named `name`.
    fn value_position(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|column| column.value() == Some(name))
    }
}

/// The rows `layer` takes, by geometry type and `where`.
fn layer_rows(columns: &[ColumnDef], layer: &PlannedLayer, exprs: &[PlannedExpr]) -> Result<Condition, Untranslatable> {
    let geometry = layer.geometry_condition();
    let Some(filter) = layer.filter else {
        return Ok(Condition::and(geometry));
    };
    let planned = &exprs[filter.index];
    let keys = columns.iter().find(|column| matches!(column, ColumnDef::Keys(_)));
    let condition = match keys {
        Some(keys) => Err(TranslateError::KeysMayReplaceColumns(keys.name().to_owned())),
        None => planned.expr.condition(columns),
    };
    let condition = condition.map_err(|reason| Untranslatable {
        layer: layer.name.clone(),
        expr: planned.source.clone(),
        reason,
    })?;
    Ok(Condition::and(geometry.into_iter().chain([condition])))
}

/// What the definitions writing into one output layer share, gathered before any of them is planned.
struct OutputDraft {
    /// The output layer's name.
    name: String,
    /// The name of its first definition.
    first: String,
    /// Indexes of its definitions in plan order.
    layers: Vec<usize>,
    /// The zooms of all its definitions.
    zooms: RangeInclusive<u8>,
    /// Their extent, and the widest of their buffers.
    grid: LayerGrid,
    /// Their encoder, which they share.
    encoder: EncoderConfig,
    /// The `sort_by` of the definitions that set one, and the name of the first of them.
    sort_by: Option<(Vec<SortDef>, String)>,
    /// The `id_order` of the definitions that set one, and the name of the first of them.
    id_order: Option<(IdOrder, String)>,
    /// Each operation, with the name and index of the definition it was taken from.
    per_tile: Vec<(TileOpDef, String, usize)>,
    /// The attributes any of its definitions hides, each once.
    hidden: Vec<String>,
}

impl OutputDraft {
    /// The output layers of `tables`, in the order their first definitions come in.
    ///
    /// # Errors
    ///
    /// If two definitions of one output layer disagree on a setting it has once.
    fn all(tables: &[TableDef]) -> Result<Vec<Self>, PlanError> {
        let mut drafts: Vec<Self> = Vec::new();
        for (index, def) in tables.iter().flat_map(|table| &table.layers).enumerate() {
            let name = def.output.as_ref().unwrap_or(&def.name);
            let Some(draft) = drafts.iter_mut().find(|draft| draft.name == *name) else {
                drafts.push(Self {
                    name: name.clone(),
                    first: def.name.clone(),
                    layers: vec![index],
                    zooms: def.zooms.clone(),
                    grid: def.grid,
                    encoder: def.encoder,
                    sort_by: (!def.sort_by.is_empty()).then(|| (def.sort_by.clone(), def.name.clone())),
                    id_order: def.id_order.map(|order| (order, def.name.clone())),
                    per_tile: def
                        .per_tile
                        .iter()
                        .map(|op| (op.clone(), def.name.clone(), index))
                        .collect(),
                    hidden: Vec::new(),
                });
                drafts.last_mut().expect("just pushed").add_hidden(def);
                continue;
            };
            draft.add(index, def)?;
            draft.add_hidden(def);
        }
        Ok(drafts)
    }

    /// Adds the attributes `def` hides that no definition before it hid.
    fn add_hidden(&mut self, def: &LayerDef) {
        for name in &def.hidden {
            if !self.hidden.contains(name) {
                self.hidden.push(name.clone());
            }
        }
    }

    /// Adds definition `def`, number `index` in the plan.
    ///
    /// # Errors
    ///
    /// If it disagrees with the definitions before it on a setting the output layer has once.
    fn add(&mut self, index: usize, def: &LayerDef) -> Result<(), PlanError> {
        let mismatch = |first: &str, setting| PlanError::OutputMismatch {
            output: self.name.clone(),
            first: first.to_owned(),
            second: def.name.clone(),
            setting,
        };
        if def.grid.extent != self.grid.extent {
            return Err(mismatch(&self.first, "extent"));
        }
        if def.encoder != self.encoder {
            return Err(mismatch(&self.first, "encoder"));
        }
        match &self.sort_by {
            Some((sort_by, by)) if !def.sort_by.is_empty() && *sort_by != def.sort_by => {
                return Err(mismatch(by, "sort_by"));
            }
            None if !def.sort_by.is_empty() => self.sort_by = Some((def.sort_by.clone(), def.name.clone())),
            _ => {}
        }
        match (&self.id_order, def.id_order) {
            (Some((order, by)), Some(other)) if *order != other => return Err(mismatch(by, "id_order")),
            (None, Some(order)) => self.id_order = Some((order, def.name.clone())),
            _ => {}
        }
        for key in TileOp::KEYS {
            let ops: Vec<&TileOpDef> = def.per_tile.iter().filter(|op| op.op.key() == key).collect();
            if ops.is_empty() {
                continue;
            }
            let mut known = self.per_tile.iter().filter(|(op, ..)| op.op.key() == key).peekable();
            if let Some((_, by, _)) = known.peek() {
                let by = by.clone();
                if !known.map(|(op, ..)| op).eq(ops) {
                    return Err(mismatch(&by, key));
                }
            } else {
                let added = ops.into_iter().map(|op| (op.clone(), def.name.clone(), index));
                self.per_tile.extend(added);
            }
        }
        self.layers.push(index);
        self.zooms = (*self.zooms.start()).min(*def.zooms.start())..=(*self.zooms.end()).max(*def.zooms.end());
        self.grid.buffer = self.grid.buffer.max(def.grid.buffer);
        Ok(())
    }

    /// The output layer of `layers`, numbering the output keys of its definitions in the output layer's, and
    /// planning its operations over them. `own_keys` holds each definition's own output keys, and whether it copies
    /// every property of a table with dynamic ones.
    fn plan(
        self,
        layers: &mut [PlannedLayer],
        own_keys: &[(Vec<String>, bool)],
        exprs: &mut Vec<PlannedExpr>,
    ) -> Result<PlannedOutput, PlanError> {
        let mut keys: Vec<String> = Vec::new();
        for &layer in &self.layers {
            let map: Vec<KeyId> = own_keys[layer]
                .0
                .iter()
                .map(|name| {
                    let pos = keys.iter().position(|key| key == name).unwrap_or_else(|| {
                        keys.push(name.clone());
                        keys.len() - 1
                    });
                    key_id(pos)
                })
                .collect();
            layers[layer].rekey(&map);
        }
        let output_props = self.layers.iter().any(|&layer| own_keys[layer].1);
        add_rank_keys(&self, &mut keys);
        check_group_by(&self, &keys, output_props)?;
        let per_tile = plan_per_tile(&self, &keys, output_props, exprs)?;
        let hidden = self
            .hidden
            .iter()
            .map(|name| {
                let pos = keys.iter().position(|key| key == name);
                key_id(pos.expect("plan_layer checks that each hidden attribute is one of its keys"))
            })
            .collect();
        Ok(PlannedOutput {
            info: LayerInfo {
                encoder: if self.sort_by.is_none() && self.id_order.is_none() {
                    self.encoder
                } else {
                    self.encoder
                        .with_id_sort(false)
                        .with_spatial_morton_sort(false)
                        .with_spatial_hilbert_sort(false)
                },
                name: self.name,
                grid: self.grid,
                per_tile,
                hidden,
            },
            layers: self.layers,
            keys,
        })
    }
}

fn key_id(pos: usize) -> KeyId {
    KeyId::new(u32::try_from(pos).expect("fewer than 2^32 keys"))
}

/// The definition as it takes features for output layer `output`, which `draft` describes, and its own output keys.
fn plan_layer(
    index: u8,
    def: LayerDef,
    output: usize,
    draft: &OutputDraft,
    compiler: &mut Compiler<'_>,
) -> Result<(PlannedLayer, Vec<String>), PlanError> {
    let geometry = if def.geometry.is_empty() {
        vec![GeometryDef::ANY]
    } else {
        def.geometry.clone()
    };
    if geometry.iter().any(|entry| entry.draws() != geometry[0].draws()) {
        return Err(PlanError::MixedGeometry { layer: def.name });
    }
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
        thinning_grid: clamp_grids(&layer_grids(draft), &def.zooms),
    };
    let cases = def
        .rules
        .iter()
        .map(|rule| plan_case(&def, rule, compiler, &mut keys, &fallback))
        .collect::<Result<_, PlanError>>()?;
    let sort_by = draft
        .sort_by
        .iter()
        .flat_map(|(keys, _)| keys)
        .map(|key| {
            let expr = compiler.compile(&key.expr)?;
            let role = SortBy {
                descending: key.descending,
            };
            Ok(compiler.add(&key.expr, expr, role))
        })
        .collect::<Result<_, PlanError>>()?;
    check_hidden(&def, &keys)?;
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
    let layer = PlannedLayer {
        name: def.name,
        output,
        render,
        zooms: def.zooms,
        geometry,
        filter,
        id,
        attributes: def.attributes,
        copy,
        cases,
        fallback,
        sort_by,
        id_order: draft.id_order.as_ref().map(|(order, _)| *order),
    };
    Ok((layer, keys))
}

/// Every `group_by` attribute of the output layer's operations is one it writes: one of `keys`, or, when a
/// definition copies every property of a table with dynamic ones (`output_props`), perhaps one of those.
fn check_group_by(draft: &OutputDraft, keys: &[String], output_props: bool) -> Result<(), PlanError> {
    if output_props {
        return Ok(());
    }
    for (op, layer, _) in draft
        .per_tile
        .iter()
        .filter(|(op, ..)| clamped_zooms(op, &draft.zooms).is_some())
    {
        let group_by = match &op.op {
            TileOp::MergePolygons { group_by, .. } => group_by.as_deref(),
            TileOp::MergeMulti { group_by, .. } => Some(group_by.as_slice()),
            TileOp::MergeLines { .. } | TileOp::Thinning { .. } | TileOp::Limit { .. } => continue,
        };
        if let Some(attribute) = group_by.into_iter().flatten().find(|name| !keys.contains(name)) {
            return Err(PlanError::UnknownGroupBy {
                layer: layer.clone(),
                op: op.op.key(),
                attribute: attribute.clone(),
            });
        }
    }
    Ok(())
}

/// Adds the attributes grid thinning writes ranks to, unless the output layer already writes them.
fn add_rank_keys(draft: &OutputDraft, keys: &mut Vec<String>) {
    for (op, ..) in draft
        .per_tile
        .iter()
        .filter(|(op, ..)| clamped_zooms(op, &draft.zooms).is_some())
    {
        if let TileOp::Thinning {
            method: ThinningMethod::Grid { rank: Some(rank), .. },
            ..
        } = &op.op
            && !keys.contains(&rank.attribute)
        {
            keys.push(rank.attribute.clone());
        }
    }
}

/// Every hidden attribute of the definition is one of its own output `keys`.
fn check_hidden(def: &LayerDef, keys: &[String]) -> Result<(), PlanError> {
    match def.hidden.iter().find(|name| !keys.contains(name)) {
        Some(name) => Err(PlanError::UnknownHidden {
            layer: def.name.clone(),
            attribute: name.clone(),
        }),
        None => Ok(()),
    }
}

/// The operation's zooms clamped into the layer's, or `None` when it runs at none of them.
fn clamped_zooms(op: &TileOpDef, zooms: &RangeInclusive<u8>) -> Option<RangeInclusive<u8>> {
    let lo = (*op.zooms.start()).max(*zooms.start());
    let hi = (*op.zooms.end()).min(*zooms.end());
    (lo <= hi).then_some(lo..=hi)
}

/// The output layer's operations in the order they run, each clamped into its zooms, without those outside them,
/// and with `thinning` keys compiled over its output attributes `keys` into `exprs`.
fn plan_per_tile(
    draft: &OutputDraft,
    keys: &[String],
    output_props: bool,
    exprs: &mut Vec<PlannedExpr>,
) -> Result<Vec<PlannedTileOp>, PlanError> {
    let mut ops = Vec::new();
    for (op, name, layer) in &draft.per_tile {
        let Some(zooms) = clamped_zooms(op, &draft.zooms) else {
            continue;
        };
        let units = f64::from(draft.grid.extent) / TILE_PIXELS;
        let buffer = |buffer: Option<f64>| buffer.map_or(f64::from(draft.grid.buffer), |px| px * units);
        let planned = match &op.op {
            TileOp::MergeLines {
                min_length,
                tolerance,
                buffer: px,
            } => PlannedOp::MergeLines(PlannedMergeLines {
                min_length: min_length * units,
                tolerance: tolerance * units,
                buffer: buffer(*px),
            }),
            TileOp::MergePolygons {
                min_area,
                min_hole_area,
                distance,
                buffer: px,
                group_by,
            } => PlannedOp::MergePolygons(PlannedMergePolygons {
                min_area: min_area * units * units,
                min_hole_area: min_hole_area * units * units,
                distance: distance * units,
                buffer: buffer(*px),
                group_by: group_by.clone(),
                warned: Arc::default(),
            }),
            TileOp::Thinning { method, keys: sources } => PlannedOp::Thinning {
                keys: sources
                    .iter()
                    .map(|source| {
                        let expr = CompiledExpr::compile(source, keys, output_props).map_err(|error| {
                            PlanError::LayerExpr {
                                layer: name.clone(),
                                error,
                            }
                        })?;
                        if expr.measures_area() {
                            return Err(PlanError::ThinningArea {
                                layer: name.clone(),
                                expr: source.clone(),
                            });
                        }
                        exprs.push(PlannedExpr {
                            expr,
                            source: source.clone(),
                            layer: *layer,
                        });
                        Ok(ExprRef::new(exprs.len() - 1, ThinningKey))
                    })
                    .collect::<Result<_, PlanError>>()?,
                method: match method {
                    ThinningMethod::Grid { limit, rank, .. } => PlannedThinning::Grid(match rank {
                        Some(rank) => {
                            if rank.offset.checked_add(i64::from(limit.get())).is_none() {
                                return Err(PlanError::RankOverflow {
                                    layer: name.clone(),
                                    attribute: rank.attribute.clone(),
                                    offset: rank.offset,
                                });
                            }
                            Some(PlannedRank {
                                key: key_id(
                                    keys.iter()
                                        .position(|key| *key == rank.attribute)
                                        .expect("OutputDraft::plan adds every rank attribute to the keys"),
                                ),
                                offset: rank.offset,
                            })
                        }
                        None => None,
                    }),
                    ThinningMethod::Distance { distance } => PlannedThinning::Distance(distance * units),
                    ThinningMethod::Tile => PlannedThinning::Tile,
                },
            },
            TileOp::MergeMulti { geometry, group_by } => PlannedOp::MergeMulti {
                geometry: geometry.clone(),
                group_by: group_by.clone(),
            },
            TileOp::Limit { count } => PlannedOp::Limit { count: *count },
        };
        ops.push((op.op.rank(), PlannedTileOp { zooms, op: planned }));
    }
    ops.sort_by_key(|(rank, op)| (*rank, *op.zooms.start()));
    Ok(ops.into_iter().map(|(_, op)| op).collect())
}

/// [`TileOp::MergeLines`] with its lengths in tile units.
#[derive(Clone, Debug)]
pub(crate) struct PlannedMergeLines {
    /// Merged lines shorter than this inside the tile and `buffer` are dropped.
    pub(crate) min_length: f64,
    /// RDP tolerance for the merged lines; 0 keeps every vertex.
    pub(crate) tolerance: f64,
    /// Around the tile, kept after merging.
    pub(crate) buffer: f64,
}

/// [`TileOp::MergePolygons`] with its lengths and areas in tile units.
#[derive(Clone, Debug)]
pub(crate) struct PlannedMergePolygons {
    pub(crate) min_area: f64,
    pub(crate) min_hole_area: f64,
    pub(crate) distance: f64,
    pub(crate) buffer: f64,
    pub(crate) group_by: Option<Vec<String>>,
    /// Whether a group that could not be merged was logged.
    pub(crate) warned: Arc<AtomicBool>,
}

/// A [`TileOpDef`] as the encoder runs it.
#[derive(Clone, Debug)]
pub struct PlannedTileOp {
    pub(crate) zooms: RangeInclusive<u8>,
    pub(crate) op: PlannedOp,
}

/// A [`TileOp`] with its expressions compiled and its distances in tile units.
#[derive(Clone, Debug)]
pub(crate) enum PlannedOp {
    MergeLines(PlannedMergeLines),
    MergePolygons(PlannedMergePolygons),
    Thinning {
        keys: Vec<ExprRef<ThinningKey>>,
        method: PlannedThinning,
    },
    MergeMulti {
        geometry: Vec<GeometryType>,
        group_by: Vec<String>,
    },
    Limit {
        count: u32,
    },
}

impl PlannedOp {
    /// Which operation this is.
    pub(crate) const fn kind(&self) -> OpKind {
        match self {
            Self::MergeLines(_) => OpKind::MergeLines,
            Self::MergePolygons(_) => OpKind::MergePolygons,
            Self::Thinning { .. } => OpKind::Thinning,
            Self::MergeMulti { .. } => OpKind::MergeMulti,
            Self::Limit { .. } => OpKind::Limit,
        }
    }
}

/// A [`ThinningMethod`] as the encoder runs it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PlannedThinning {
    /// Rendering put each point in its square and gave it its limit.
    Grid(Option<PlannedRank>),
    /// In tile units.
    Distance(f64),
    Tile,
}

/// A [`GridRank`] with its attribute's key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PlannedRank {
    pub(crate) key: KeyId,
    pub(crate) offset: i64,
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

fn check_duplicates<'a>(layer: &str, keys: impl IntoIterator<Item = &'a String>) -> Result<(), PlanError> {
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
            let pos = keys.iter().position(|key| *key == attr.name).unwrap_or_else(|| {
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
        thinning_grid: rule.thinning.as_ref().map_or_else(
            || fallback.thinning_grid.clone(),
            |grids| clamp_grids(grids, &def.zooms),
        ),
    })
}

/// The grids of the output layer's grid thinning.
fn layer_grids(draft: &OutputDraft) -> Vec<ThinningGrid> {
    draft
        .per_tile
        .iter()
        .filter_map(|(op, ..)| {
            let TileOp::Thinning {
                method: ThinningMethod::Grid { size, limit, .. },
                ..
            } = &op.op
            else {
                return None;
            };
            Some(ThinningGrid {
                zooms: op.zooms.clone(),
                size: *size,
                limit: *limit,
            })
        })
        .collect()
}

/// `grids` clamped into `zooms`, without those outside them.
fn clamp_grids(grids: &[ThinningGrid], zooms: &RangeInclusive<u8>) -> Vec<ThinningGrid> {
    grids
        .iter()
        .filter_map(|grid| {
            let lo = (*grid.zooms.start()).max(*zooms.start());
            let hi = (*grid.zooms.end()).min(*zooms.end());
            (lo <= hi).then(|| ThinningGrid {
                zooms: lo..=hi,
                ..grid.clone()
            })
        })
        .collect()
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
            let column = expr.as_property().and_then(|name| compiler.value_position(name));
            match column {
                Some(column) => PlannedValue::Column(key_id(column)),
                None => PlannedValue::Expr(compiler.add(source, expr, Attr)),
            }
        }
    })
}

/// The output layer's known keys, and the output key of each table column copied as it is.
fn layer_keys(def: &LayerDef, compiler: &Compiler<'_>) -> Result<(Vec<String>, Vec<Option<KeyId>>), PlanError> {
    let columns = compiler.columns;
    let (keys, copied_from) = match &def.attributes {
        AttributesDef::All => (columns.iter().map(|c| c.name().to_owned()).collect(), 0),
        AttributesDef::None => (Vec::new(), 0),
        AttributesDef::Columns(keys) => {
            check_duplicates(&def.name, keys)?;
            if !compiler.dynamic_props
                && let Some(key) = keys.iter().find(|key| !compiler.values.contains(&key.as_str()))
            {
                return Err(PlanError::UnknownColumn {
                    layer: def.name.clone(),
                    column: key.clone(),
                });
            }
            (keys.clone(), 0)
        }
        AttributesDef::Computed { attributes, prefixes } => {
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
