//! The paths a feature can take through a layer, with the rows that take each.

use std::fmt::{self, Display, Formatter};

use cel::common::ast::{CallExpr, Expr, IdedExpr, LiteralValue, operators};
use cel::common::types::{CelBool, CelDouble, CelInt, CelString, CelUInt};
use cel::{ExecutionError, Value};
use geo::BoundingRect;
use geo_types::Rect;

use crate::error::TileGenResult;
use crate::expr::{Cel, FEATURE, FeatureView, condition_of, eval_part, feature_key};
use crate::phase1::project_geometry;
use crate::plan::{
    Case, ColumnDef, CompareOp, Condition, ExprRef, Filter, GeometryType, Literal, Plan, PlannedAttrs, PlannedExpr,
    PlannedId, PlannedLayer, PlannedValue, PlannedZoom, Polarity, Term, TranslateError, Untranslatable, ValueKind,
};
use crate::props::SourceStr;
use crate::source::{Crs, SourceFeature};
use crate::{FeatureGeom, PixelThreshold, TileGenError};

/// One path through a layer.
#[derive(Clone, Debug, PartialEq)]
pub struct Branch {
    pub layer: String,
    /// What a feature on this path meets, e.g. ``rule 2 `class == 'path'` ``.
    pub path: String,
    /// The rows on this path, or why a source cannot select them.
    pub rows: Result<Condition, Untranslatable>,
    /// Only the rows whose geometry has this size, which no [`Condition`] can express.
    pub size: Option<SizeBand>,
}

impl Display for Branch {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.layer, self.path)
    }
}

/// Lines and polygons whose bounding box's longer side is `min` to below `max` pixels of a 256-pixel tile
/// at `zoom`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SizeBand {
    pub zoom: u8,
    pub min: f64,
    pub max: f64,
}

impl SizeBand {
    /// Whether a geometry with the bounding box `bbox`, in the renderer's unit square, is in this band.
    fn holds(self, bbox: Rect<f64>) -> bool {
        let size = bbox.width().max(bbox.height()) * 256.0 * f64::from(1u32 << self.zoom);
        self.min <= size && size < self.max
    }
}

/// The only values a column of a table takes, `null` not among them, known from outside the plan, e.g. from the
/// source's filter.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnValues {
    pub column: String,
    pub values: Vec<Literal>,
}

impl Plan {
    /// Every path through the layers of table `table` in layer order, each with rows that may take other paths too.
    /// A column in `known` that a layer's expressions read gives that layer one path per value.
    ///
    /// # Errors
    ///
    /// If the plan has no table `table`.
    pub fn branches(&self, table: u16, known: &[ColumnValues]) -> TileGenResult<Vec<Branch>> {
        Ok(self.paths(table, known)?.into_iter().map(|path| path.branch).collect())
    }

    /// The paths of [`Self::branches`] that `feature` takes as the layers evaluate it, its geometry in `crs`.
    ///
    /// # Errors
    ///
    /// If the plan has no table `table`.
    pub fn paths_of(
        &self,
        table: u16,
        known: &[ColumnValues],
        crs: Crs,
        feature: &SourceFeature,
    ) -> TileGenResult<Vec<Branch>> {
        let paths = self.paths(table, known)?;
        let mut geometry = feature.geometry.clone();
        project_geometry(crs, &mut geometry);
        let Some(geom) = geometry.feature_geom() else {
            return Ok(Vec::new());
        };
        let mut errors = vec![0; self.exprs.len()];
        Ok(self.with_view(usize::from(table), feature, |view| {
            paths
                .into_iter()
                .filter(|path| path.test.passes(self, view, geom, &mut errors))
                .map(|path| path.branch)
                .collect()
        }))
    }

    /// The paths through the layers of table `table`, without a second path with the rows of an earlier one.
    fn paths(&self, table: u16, known: &[ColumnValues]) -> TileGenResult<Vec<Path<'_>>> {
        let planned = self
            .tables
            .get(usize::from(table))
            .ok_or(TileGenError::UnknownTable(table))?;
        let mut out: Vec<Path<'_>> = Vec::new();
        for layer_index in planned.layers.clone() {
            let paths = LayerPaths {
                columns: &planned.defs,
                exprs: &self.exprs,
                layer: &self.layers[layer_index],
                layer_index,
                known,
            };
            for path in paths.all() {
                let branch = &path.branch;
                let seen = branch.rows.is_ok()
                    && out.iter().any(|p| {
                        p.branch.layer == branch.layer && p.branch.rows == branch.rows && p.branch.size == branch.size
                    });
                if !seen {
                    out.push(path);
                }
            }
        }
        Ok(out)
    }
}

/// A path, and what a feature meets to take it.
struct Path<'p> {
    branch: Branch,
    test: Test<'p>,
}

/// What a feature on a path meets, as the evaluator decides it.
struct Test<'p> {
    layer: &'p PlannedLayer,
    reach: Reach,
    /// Parts of expressions, each with the value it has on the path.
    checks: Vec<(IdedExpr, Want)>,
    size: Option<SizeBand>,
}

impl Test<'_> {
    /// Whether the feature seen through `view`, with the projected geometry `geom`, meets the test.
    fn passes(
        &self,
        plan: &Plan,
        view: &FeatureView<'_, SourceStr>,
        geom: FeatureGeom<'_>,
        errors: &mut [u64],
    ) -> bool {
        let layer = self.layer;
        if layer.geometry.is_some_and(|kind| !kind.matches(geom)) {
            return false;
        }
        let reached = match &self.reach {
            Reach::Geometry => true,
            Reach::Layer => plan.matched_case(layer, view, errors).is_some(),
            Reach::Cases(cases) => plan
                .matched_case(layer, view, errors)
                .is_some_and(|case| cases.contains(&case_index(layer, case))),
        };
        reached
            && self.checks.iter().all(|(part, want)| want.holds(eval_part(part, view)))
            && self.size.is_none_or(|size| {
                !matches!(geom, FeatureGeom::Points(_)) && bounding_box(geom).is_some_and(|bbox| size.holds(bbox))
            })
    }
}

/// The features a test looks at.
#[derive(Clone, Debug, PartialEq)]
enum Reach {
    /// Those of the layer's geometry type.
    Geometry,
    /// Those the layer's `where` takes.
    Layer,
    /// Those one of these cases takes, by index into the layer's rules, `None` for no rule.
    Cases(Vec<Option<usize>>),
}

/// The value a part of an expression has on a path.
#[derive(Clone, Copy, Debug)]
enum Want {
    True,
    False,
    Null,
    NotNull,
}

impl Want {
    /// Whether `value` is the wanted one, never when the part fails.
    fn holds(self, value: Result<Value, ExecutionError>) -> bool {
        let Ok(value) = value else {
            return false;
        };
        match self {
            Self::True => matches!(value, Value::Bool(true)),
            Self::False => matches!(value, Value::Bool(false)),
            Self::Null => matches!(value, Value::Null),
            Self::NotNull => !matches!(value, Value::Null),
        }
    }
}

/// The index of `case` among the rules of `layer`, `None` for the layer's own settings.
fn case_index(layer: &PlannedLayer, case: &Case) -> Option<usize> {
    layer.cases.iter().position(|c| std::ptr::eq(c, case))
}

/// The bounding box of `geom`.
fn bounding_box(geom: FeatureGeom<'_>) -> Option<Rect<f64>> {
    let union = |a: Rect<f64>, b: Rect<f64>| {
        Rect::new(
            geo_types::coord! { x: a.min().x.min(b.min().x), y: a.min().y.min(b.min().y) },
            geo_types::coord! { x: a.max().x.max(b.max().x), y: a.max().y.max(b.max().y) },
        )
    };
    match geom {
        FeatureGeom::Points(points) => points.iter().map(|p| Rect::new(*p, *p)).reduce(union),
        FeatureGeom::Lines(lines) => lines.iter().filter_map(BoundingRect::bounding_rect).reduce(union),
        FeatureGeom::Polygons(polygons) => polygons.iter().filter_map(BoundingRect::bounding_rect).reduce(union),
    }
}

/// Finds the paths through one layer.
struct LayerPaths<'a, 'k> {
    columns: &'a [ColumnDef],
    exprs: &'a [PlannedExpr],
    layer: &'a PlannedLayer,
    layer_index: usize,
    known: &'k [ColumnValues],
}

/// A part of a path's rows: the expression it comes from, and its condition.
type Part<'a> = (&'a str, Result<Condition, TranslateError>);

impl<'a> LayerPaths<'a, '_> {
    /// The layer's paths: its `where`, its rules, the choices in its expressions, the known values of the columns
    /// they read, and its size limits.
    fn all(&self) -> Vec<Path<'a>> {
        let mut out = Vec::new();
        match self.layer.filter.map(|f| self.expr(f)) {
            Some(filter) => self.filter(filter, &mut out),
            None if self.layer.cases.is_empty() => {
                out.push(self.path("every feature".to_owned(), self.base(), self.test(Reach::Layer)));
            }
            None => {}
        }
        self.rules(&mut out);
        self.expressions(&mut out);
        self.values(&mut out);
        self.sizes(&mut out);
        out
    }

    /// The rows `where` takes, those it does not, and those it drops for a `null` column.
    fn filter(&self, filter: &'a PlannedExpr, out: &mut Vec<Path<'a>>) {
        let source = &filter.source;
        out.push(self.path(format!("where `{source}`"), self.base(), self.test(Reach::Layer)));
        let mut parts = self.geometry();
        parts.push(self.part(filter, Polarity::Negated));
        let mut test = self.test(Reach::Geometry);
        test.checks.push((filter.expr.ast().clone(), Want::False));
        out.push(self.path(format!("not where `{source}`"), parts, test));
        for column in filter.expr.columns().filter(|c| self.may_be_null(c)) {
            let mut parts = self.geometry();
            parts.push((source.as_str(), Ok(column_is_null(column))));
            let mut test = self.test(Reach::Geometry);
            test.checks.push((property(column), Want::Null));
            out.push(self.path(format!("where `{source}` with `{column}` null"), parts, test));
        }
    }

    /// The first rule that matches takes the feature, so a rule's rows fail every earlier rule.
    fn rules(&self, out: &mut Vec<Path<'a>>) {
        for (i, case) in self.layer.cases.iter().enumerate() {
            let path = match case.when {
                Some(when) => format!("rule {} `{}`", i + 1, self.expr(when).source),
                None => format!("rule {}, every other feature", i + 1),
            };
            out.push(self.path(path, self.case_rows(Some(i)), self.test(Reach::Cases(vec![Some(i)]))));
            if case.when.is_none() {
                return;
            }
        }
        if !self.layer.cases.is_empty() {
            out.push(self.path(
                "no rule".to_owned(),
                self.case_rows(None),
                self.test(Reach::Cases(vec![None])),
            ));
        }
    }

    /// Both sides of each `? :`, and each argument of a `coalesce` being the first that is not `null`, on the rows of
    /// the rules that evaluate the expression.
    fn expressions(&self, out: &mut Vec<Path<'a>>) {
        let filter = self.layer.filter.map(|f| f.index);
        let whens: Vec<usize> = self
            .layer
            .cases
            .iter()
            .filter_map(|c| c.when.map(|w| w.index))
            .collect();
        for (index, planned) in self.exprs.iter().enumerate() {
            if planned.layer != self.layer_index || filter == Some(index) || whens.contains(&index) {
                continue;
            }
            let source = planned.source.as_str();
            let reach = self.reach_of(index);
            let base = || self.reached_rows(&reach);
            for choice in choices(planned.expr.ast()) {
                match choice {
                    Choice::Test(test) => {
                        for (polarity, want, side) in [
                            (Polarity::AsWritten, Want::True, "true"),
                            (Polarity::Negated, Want::False, "false"),
                        ] {
                            let mut parts = base();
                            parts.push((source, self.translate(test, polarity)));
                            let mut check = self.test(reach.clone());
                            check.checks.push((test.clone(), want));
                            out.push(self.path(format!("`{source}`: `{}` {side}", Cel(test)), parts, check));
                        }
                        for column in read_columns(test).into_iter().filter(|c| self.may_be_null(c)) {
                            let mut parts = base();
                            parts.push((source, Ok(column_is_null(&column))));
                            let mut check = self.test(reach.clone());
                            check.checks.push((property(&column), Want::Null));
                            let path = format!("`{source}`: `{}` fails with `{column}` null", Cel(test));
                            out.push(self.path(path, parts, check));
                        }
                    }
                    Choice::Coalesce(args) => {
                        for (k, arg) in args.iter().enumerate() {
                            let mut parts = base();
                            parts.extend(args[..k].iter().map(|a| (source, self.is_null(a, Polarity::AsWritten))));
                            parts.push((source, self.is_null(arg, Polarity::Negated)));
                            let mut check = self.test(reach.clone());
                            check.checks.extend(args[..k].iter().map(|a| (a.clone(), Want::Null)));
                            check.checks.push((arg.clone(), Want::NotNull));
                            let path = format!("`{source}`: `{}` is the first not null", Cel(arg));
                            out.push(self.path(path, parts, check));
                        }
                        let mut parts = base();
                        parts.extend(args.iter().map(|a| (source, self.is_null(a, Polarity::AsWritten))));
                        let mut check = self.test(reach.clone());
                        check.checks.extend(args.iter().map(|a| (a.clone(), Want::Null)));
                        out.push(self.path(format!("`{source}`: all null"), parts, check));
                    }
                }
            }
        }
    }

    /// Each known value of a column the layer's expressions read.
    fn values(&self, out: &mut Vec<Path<'a>>) {
        let read: Vec<String> = self
            .exprs
            .iter()
            .filter(|planned| planned.layer == self.layer_index)
            .flat_map(|planned| read_columns(planned.expr.ast()))
            .collect();
        for known in self.known.iter().filter(|k| read.contains(&k.column)) {
            for value in &known.values {
                let Some(kind) = self.compared_as(&known.column, value) else {
                    continue;
                };
                let path = format!("`{}` is {}", known.column, LiteralCel(value));
                let mut parts = self.base();
                parts.push(("the source's filter", self.equals(&known.column, value, kind)));
                let mut test = self.test(Reach::Layer);
                test.checks.push((equals(&known.column, value), Want::True));
                out.push(self.path(path, parts, test));
            }
        }
    }

    /// For the rules of each `min_size`, lines and polygons kept only at the layer's max zoom, and those too small for
    /// every zoom.
    fn sizes(&self, out: &mut Vec<Path<'a>>) {
        if self.layer.geometry == Some(GeometryType::Point) {
            return;
        }
        let mut groups: Vec<(PixelThreshold, Vec<Option<usize>>)> = Vec::new();
        for case in self.reachable() {
            let min_size = self.case(case).min_size;
            match groups.iter_mut().find(|(size, _)| *size == min_size) {
                Some((_, cases)) => cases.push(case),
                None => groups.push((min_size, vec![case])),
            }
        }
        let named = groups.len() > 1;
        for (min_size, cases) in groups {
            let reach = if named { Reach::Cases(cases) } else { Reach::Layer };
            let prefix = match &reach {
                Reach::Cases(cases) => format!("{}: ", CaseNames(cases)),
                Reach::Geometry | Reach::Layer => String::new(),
            };
            let mut parts = self.reached_rows(&reach);
            if self.layer.geometry.is_none() {
                let shapes = [GeometryType::Line, GeometryType::Polygon].map(Condition::Geometry);
                parts.push(("geometry", Ok(Condition::or(shapes))));
            }
            let mut band = |path: String, size: SizeBand| {
                let mut test = self.test(reach.clone());
                test.size = Some(size);
                let mut path = self.path(format!("{prefix}{path}"), parts.clone(), test);
                path.branch.size = Some(size);
                out.push(path);
            };
            let (min, max) = (*self.layer.zooms.start(), *self.layer.zooms.end());
            let at_max = min_size.at_max_zoom;
            let below = min_size.below_max_zoom;
            if min < max && at_max < 2.0 * below {
                let size = SizeBand {
                    zoom: max,
                    min: at_max,
                    max: 2.0 * below,
                };
                band(format!("kept only at z{max} by `min_size_at_maxzoom`"), size);
            }
            if at_max > 0.0 {
                let size = SizeBand {
                    zoom: max,
                    min: 0.0,
                    max: at_max,
                };
                band("smaller than `min_size` at every zoom".to_owned(), size);
            }
        }
    }

    /// The cases a feature can reach: the rules up to the first without `when`, and no rule if every rule has one.
    fn reachable(&self) -> Vec<Option<usize>> {
        let cases = &self.layer.cases;
        match cases.iter().position(|case| case.when.is_none()) {
            Some(last) => (0..=last).map(Some).collect(),
            None => (0..cases.len()).map(Some).chain([None]).collect(),
        }
    }

    /// The case at `index`, the layer's own settings for `None`.
    fn case(&self, index: Option<usize>) -> &'a Case {
        index.map_or(&self.layer.fallback, |i| &self.layer.cases[i])
    }

    /// The cases that evaluate expression `index`, all of them for the layer's `id` and `sort_by`.
    fn reach_of(&self, index: usize) -> Reach {
        let layer_wide = matches!(self.layer.id, PlannedId::Expr(id) if id.index == index)
            || self.layer.sort_by.iter().any(|s| s.index == index);
        let reachable = self.reachable();
        let cases: Vec<Option<usize>> = reachable
            .iter()
            .copied()
            .filter(|&case| layer_wide || case_exprs(self.case(case)).any(|e| e == index))
            .collect();
        if cases == reachable {
            Reach::Layer
        } else {
            Reach::Cases(cases)
        }
    }

    /// The rows of the features of `reach`.
    fn reached_rows(&self, reach: &Reach) -> Vec<Part<'a>> {
        match reach {
            Reach::Geometry => self.geometry(),
            Reach::Layer => self.base(),
            Reach::Cases(cases) => {
                let mut parts = self.base();
                let mut any = Vec::with_capacity(cases.len());
                for &case in cases {
                    let mut all = Vec::new();
                    for (source, part) in self.case_rows(case).into_iter().skip(parts.len()) {
                        match part {
                            Ok(condition) => all.push(condition),
                            Err(reason) => {
                                parts.push((source, Err(reason)));
                                return parts;
                            }
                        }
                    }
                    any.push(Condition::and(all));
                }
                parts.push(("rules", Ok(Condition::or(any))));
                parts
            }
        }
    }

    /// The rows rule `index` takes, which fail every earlier rule, or those no rule takes for `None`.
    fn case_rows(&self, index: Option<usize>) -> Vec<Part<'a>> {
        let mut parts = self.base();
        let earlier = index.unwrap_or(self.layer.cases.len());
        for case in &self.layer.cases[..earlier] {
            if let Some(when) = case.when {
                parts.push(self.part(self.expr(when), Polarity::Negated));
            }
        }
        if let Some(when) = index.and_then(|i| self.layer.cases[i].when) {
            parts.push(self.part(self.expr(when), Polarity::AsWritten));
        }
        parts
    }

    /// The layer's geometry type and `where`.
    fn base(&self) -> Vec<Part<'a>> {
        let mut parts = self.geometry();
        if let Some(filter) = self.layer.filter {
            parts.push(self.part(self.expr(filter), Polarity::AsWritten));
        }
        parts
    }

    /// The layer's geometry type, if it has one.
    fn geometry(&self) -> Vec<Part<'a>> {
        self.layer
            .geometry
            .map(|geometry| ("geometry", Ok(Condition::Geometry(geometry))))
            .into_iter()
            .collect()
    }

    /// `planned` as a part of `polarity`.
    fn part(&self, planned: &'a PlannedExpr, polarity: Polarity) -> Part<'a> {
        (planned.source.as_str(), self.translate(planned.expr.ast(), polarity))
    }

    /// `expr == null` as a condition of `polarity`.
    fn is_null(&self, expr: &IdedExpr, polarity: Polarity) -> Result<Condition, TranslateError> {
        let null = IdedExpr {
            id: 0,
            expr: Expr::Literal(LiteralValue::Null),
        };
        self.translate(&call(operators::EQUALS, vec![expr.clone(), null]), polarity)
    }

    /// `expr` as a condition of `polarity`, which no source can check while a `jsonb` column may replace
    /// the columns it reads.
    fn translate(&self, expr: &IdedExpr, polarity: Polarity) -> Result<Condition, TranslateError> {
        if let Some(keys) = self.columns.iter().find(|c| matches!(c, ColumnDef::Keys(_))) {
            return Err(TranslateError::KeysMayReplaceColumns(keys.name().to_owned()));
        }
        condition_of(expr, self.columns, polarity)
    }

    /// The kind `column` and `value` compare as, or `None` when the column reads as values of another kind, e.g. a
    /// `numeric` column read as text, so expressions never see `value` there.
    fn compared_as(&self, column: &str, value: &Literal) -> Option<ValueKind> {
        let kind = match value {
            Literal::Bool(_) => ValueKind::Bool,
            Literal::Int(_) | Literal::UInt(_) => ValueKind::Int,
            Literal::Float(_) => ValueKind::Float,
            Literal::String(_) => ValueKind::String,
        };
        let read_as = self.columns.iter().find_map(|c| match c {
            ColumnDef::Value(name, read_as) if name == column => *read_as,
            ColumnDef::Value(..) | ColumnDef::Keys(_) => None,
        });
        read_as.map_or(Some(kind), |read_as| read_as.common(kind))
    }

    /// `column == value` compared as `kind`, which no source can check while a `jsonb` column may replace the columns
    /// it reads.
    fn equals(&self, column: &str, value: &Literal, kind: ValueKind) -> Result<Condition, TranslateError> {
        if let Some(keys) = self.columns.iter().find(|c| matches!(c, ColumnDef::Keys(_))) {
            return Err(TranslateError::KeysMayReplaceColumns(keys.name().to_owned()));
        }
        Ok(Condition::Compare {
            left: Term::Column(column.to_owned()),
            op: CompareOp::Eq,
            right: Term::Literal(value.clone()),
            kind,
        })
    }

    /// A test on the features of `reach`, with no checks yet.
    fn test(&self, reach: Reach) -> Test<'a> {
        Test {
            layer: self.layer,
            reach,
            checks: Vec::new(),
            size: None,
        }
    }

    /// The path `path` with the rows meeting every part, or why the first untranslatable part cannot, and `test`.
    fn path(&self, path: String, parts: Vec<Part<'_>>, test: Test<'a>) -> Path<'a> {
        let rows = parts
            .into_iter()
            .map(|(source, part)| {
                part.map_err(|reason| Untranslatable {
                    layer: self.layer.info.name.clone(),
                    expr: source.to_owned(),
                    reason,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Condition::and);
        Path {
            branch: Branch {
                layer: self.layer.info.name.clone(),
                path,
                rows,
                size: None,
            },
            test,
        }
    }

    /// The planned expression `filter` refers to.
    fn expr(&self, filter: ExprRef<Filter>) -> &'a PlannedExpr {
        &self.exprs[filter.index]
    }

    /// Whether `name` is a column read by value that no known values rule out being `null`.
    fn may_be_null(&self, name: &str) -> bool {
        self.is_value(name) && !self.known.iter().any(|k| k.column == name)
    }

    /// Whether `name` is a column read by value.
    fn is_value(&self, name: &str) -> bool {
        self.columns
            .iter()
            .any(|c| matches!(c, ColumnDef::Value(n, _) if n == name))
    }
}

/// The cases of a layer, e.g. `rules 1, 3, no rule`.
struct CaseNames<'a>(&'a [Option<usize>]);

impl Display for CaseNames<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let rules: Vec<String> = self.0.iter().flatten().map(|i| (i + 1).to_string()).collect();
        match rules.as_slice() {
            [] => {}
            [rule] => write!(f, "rule {rule}")?,
            rules => write!(f, "rules {}", rules.join(", "))?,
        }
        if self.0.contains(&None) {
            f.write_str(if rules.is_empty() { "no rule" } else { ", no rule" })?;
        }
        Ok(())
    }
}

/// The expressions `case` evaluates besides its `when`.
fn case_exprs(case: &Case) -> impl Iterator<Item = usize> + '_ {
    let zoom = |zoom: PlannedZoom| match zoom {
        PlannedZoom::Fixed(_) => [None, None],
        PlannedZoom::Expr(expr, fallback) => [Some(expr.index), fallback.map(|f| f.index)],
    };
    let PlannedAttrs { computed, banded, .. } = &case.attrs;
    zoom(case.minzoom)
        .into_iter()
        .chain(zoom(case.maxzoom))
        .flatten()
        .chain(computed.iter().chain(banded).filter_map(|attr| match attr.value {
            PlannedValue::Expr(expr) => Some(expr.index),
            PlannedValue::Literal(_) | PlannedValue::Column(_) => None,
        }))
}

/// A literal as CEL writes it.
struct LiteralCel<'a>(&'a Literal);

impl Display for LiteralCel<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.0 {
            Literal::Bool(v) => write!(f, "{v}"),
            Literal::Int(v) => write!(f, "{v}"),
            Literal::UInt(v) => write!(f, "{v}u"),
            Literal::Float(v) => write!(f, "{v:?}"),
            Literal::String(v) => write!(f, "'{}'", v.replace('\\', "\\\\").replace('\'', "\\'")),
        }
    }
}

/// `column IS NULL`.
fn column_is_null(column: &str) -> Condition {
    Condition::IsNull {
        term: Term::Column(column.to_owned()),
        polarity: Polarity::AsWritten,
    }
}

/// The call of `func` on `args`.
fn call(func: &str, args: Vec<IdedExpr>) -> IdedExpr {
    IdedExpr {
        id: 0,
        expr: Expr::Call(CallExpr {
            func_name: func.to_owned(),
            target: None,
            args,
        }),
    }
}

/// `feature['column']`, `null` where the feature has no such property.
fn property(column: &str) -> IdedExpr {
    let feature = IdedExpr {
        id: 0,
        expr: Expr::Ident(FEATURE.to_owned()),
    };
    let key = IdedExpr {
        id: 0,
        expr: Expr::Literal(LiteralValue::String(CelString::from(column.to_owned()))),
    };
    call(operators::INDEX, vec![feature, key])
}

/// `feature['column'] == value`.
fn equals(column: &str, value: &Literal) -> IdedExpr {
    let literal = match value {
        Literal::Bool(v) => LiteralValue::Boolean(CelBool::from(*v)),
        Literal::Int(v) => LiteralValue::Int(CelInt::from(*v)),
        Literal::UInt(v) => LiteralValue::UInt(CelUInt::from(*v)),
        Literal::Float(v) => LiteralValue::Double(CelDouble::from(*v)),
        Literal::String(v) => LiteralValue::String(CelString::from(v.clone())),
    };
    let literal = IdedExpr {
        id: 0,
        expr: Expr::Literal(literal),
    };
    call(operators::EQUALS, vec![property(column), literal])
}

/// The properties `expr` reads by name, in order of first appearance.
fn read_columns(expr: &IdedExpr) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    walk(expr, &mut |e| {
        let name = if let Expr::Ident(name) = &e.expr
            && name != FEATURE
        {
            name.as_str()
        } else if let Expr::Call(call) = &e.expr
            && let Some(key) = feature_key(call)
            && let Expr::Literal(LiteralValue::String(key)) = &key.expr
        {
            key.inner()
        } else {
            return true;
        };
        if !out.iter().any(|c| c == name) {
            out.push(name.to_owned());
        }
        false
    });
    out
}

/// A place where an expression picks one of several values.
enum Choice<'e> {
    /// The test of a `? :`.
    Test(&'e IdedExpr),
    /// The arguments of `coalesce`.
    Coalesce(&'e [IdedExpr]),
}

/// The choices in `expr`, outermost first.
fn choices(expr: &IdedExpr) -> Vec<Choice<'_>> {
    let mut out = Vec::new();
    walk(expr, &mut |e| {
        if let Expr::Call(call) = &e.expr {
            match (call.func_name.as_str(), call.target.as_deref(), call.args.as_slice()) {
                (operators::CONDITIONAL, None, [test, ..]) => out.push(Choice::Test(test)),
                ("coalesce", None, args) if args.len() > 1 => out.push(Choice::Coalesce(args)),
                _ => {}
            }
        }
        true
    });
    out
}

/// Visits `expr` and its sub-expressions outermost first, skipping those below a node `visit` returns `false` for.
fn walk<'e, F: FnMut(&'e IdedExpr) -> bool>(expr: &'e IdedExpr, visit: &mut F) {
    if !visit(expr) {
        return;
    }
    match &expr.expr {
        Expr::Call(call) => {
            if let Some(target) = call.target.as_deref() {
                walk(target, visit);
            }
            for arg in &call.args {
                walk(arg, visit);
            }
        }
        Expr::Select(select) => walk(&select.operand, visit),
        Expr::List(list) => {
            for element in &list.elements {
                walk(element, visit);
            }
        }
        Expr::Unspecified
        | Expr::Ident(_)
        | Expr::Literal(_)
        | Expr::Comprehension(_)
        | Expr::Map(_)
        | Expr::Struct(_) => {}
    }
}
