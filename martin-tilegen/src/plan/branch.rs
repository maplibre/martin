//! The paths a feature can take through a layer, with the rows that take each.

use std::fmt::{self, Display, Formatter};

use cel::common::ast::{CallExpr, Expr, IdedExpr, LiteralValue, operators};
use cel::common::types::{CelBool, CelDouble, CelInt, CelString, CelUInt};
use cel::{ExecutionError, Value};
use geo::BoundingRect;
use geo_types::{Coord, Rect};

use crate::error::TileGenResult;
use crate::expr::{Cel, FEATURE, FeatureView, condition_of, eval_part, feature_key};
use crate::phase1::project_geometry;
use crate::plan::{
    Case, ColumnDef, CompareOp, Condition, ExprRef, Filter, GeometryType, Literal, Plan, PlannedAttrs, PlannedExpr,
    PlannedId, PlannedLayer, PlannedValue, PlannedZoom, Polarity, Term, TranslateError, Untranslatable, ValueKind,
    key_id,
};
use crate::props::{Prop, SourceStr};
use crate::source::{Crs, Geometry, SourceFeature};
use crate::{FeatureGeom, PixelThreshold, TileGenError};

/// One path through a layer.
#[derive(Clone, Debug, PartialEq)]
pub struct Branch {
    pub layer: String,
    /// What a feature on this path meets and what the layer makes of it, e.g. ``minzoom 6, `class == 'primary'` is
    /// true``.
    pub path: String,
    /// The rows on this path, or why a source cannot select them.
    pub rows: Result<Condition, Untranslatable>,
    /// Only the rows whose geometry has this shape, which no [`Condition`] can express.
    pub shape: Option<Shape>,
}

impl Display for Branch {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.layer, self.path)
    }
}

/// The geometries of a path's rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// Lines and polygons in a size band.
    Size(SizeBand),
    /// Lines and polygons whose bounding box crosses an edge of the tiles at this zoom.
    CrossesTileEdge(u8),
}

/// Lines and polygons whose bounding box's longer side is `min` to below `max` pixels of a 256-pixel tile
/// at `zoom`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SizeBand {
    pub zoom: u8,
    pub min: f64,
    pub max: f64,
}

impl Shape {
    /// Whether a geometry with the bounding box `bbox`, in the renderer's unit square, has this shape.
    fn holds(self, bbox: Rect<f64>) -> bool {
        let scale = |zoom: u8| f64::from(1u32 << zoom);
        match self {
            Self::Size(SizeBand { zoom, min, max }) => {
                let size = bbox.width().max(bbox.height()) * 256.0 * scale(zoom);
                min <= size && size < max
            }
            Self::CrossesTileEdge(zoom) => {
                let cell = |v: f64| (v * scale(zoom)).floor();
                let (lo, hi) = (bbox.min(), bbox.max());
                cell(lo.x).total_cmp(&cell(hi.x)).is_ne() || cell(lo.y).total_cmp(&cell(hi.y)).is_ne()
            }
        }
    }
}

/// What is known from outside the plan about the values a column of a table takes, e.g. from the source's filter or
/// the table's constraints.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnFacts {
    pub column: String,
    /// Whether the column is never `null`.
    pub not_null: bool,
    /// The only values the column takes besides `null`, any value when `None`.
    pub values: Option<Vec<Literal>>,
}

/// A column with known values that some expression can see, and whether it may be `null` too.
struct KnownValues {
    column: String,
    values: Vec<Literal>,
    nullable: bool,
}

impl KnownValues {
    /// The value of the column when it takes only one, never `null`.
    fn single(&self) -> Option<&Literal> {
        match self.values.as_slice() {
            [only] if !self.nullable => Some(only),
            _ => None,
        }
    }
}

impl Plan {
    /// Every path through the layers of table `table` in layer order, each with rows that may take other paths too.
    /// A column in `facts` with known values that a layer's expressions read gives that layer one path per value, and
    /// one for `null` unless the column is never `null`, naming what the layer makes of it wherever the value decides
    /// that, and the choices it decides get no paths of their own. A column that is never `null` gets no `null` paths.
    ///
    /// # Errors
    ///
    /// If the plan has no table `table`.
    pub fn branches(&self, table: u16, facts: &[ColumnFacts]) -> TileGenResult<Vec<Branch>> {
        Ok(self.paths(table, facts)?.into_iter().map(|path| path.branch).collect())
    }

    /// The paths of [`Self::branches`] that `feature` takes as the layers evaluate it, its geometry in `crs`.
    ///
    /// # Errors
    ///
    /// If the plan has no table `table`.
    pub fn paths_of(
        &self,
        table: u16,
        facts: &[ColumnFacts],
        crs: Crs,
        feature: &SourceFeature,
    ) -> TileGenResult<Vec<Branch>> {
        let paths = self.paths(table, facts)?;
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
    fn paths(&self, table: u16, facts: &[ColumnFacts]) -> TileGenResult<Vec<Path<'_>>> {
        let planned = self
            .tables
            .get(usize::from(table))
            .ok_or(TileGenError::UnknownTable(table))?;
        let known: Vec<KnownValues> = facts
            .iter()
            .filter_map(|f| {
                let values: Vec<Literal> = (f.values.as_deref()?.iter())
                    .filter(|value| compared_as(&planned.defs, &f.column, value).is_some())
                    .cloned()
                    .collect();
                (!values.is_empty()).then(|| KnownValues {
                    column: f.column.clone(),
                    values,
                    nullable: !f.not_null,
                })
            })
            .collect();
        let mut out: Vec<Path<'_>> = Vec::new();
        for layer_index in planned.layers.clone() {
            let paths = LayerPaths {
                plan: self,
                table: usize::from(table),
                columns: &planned.defs,
                exprs: &self.exprs,
                layer: &self.layers[layer_index],
                layer_index,
                known: &known,
                facts,
            };
            for path in paths.all() {
                let branch = &path.branch;
                let seen = branch.rows.is_ok()
                    && out.iter().any(|p| {
                        p.branch.layer == branch.layer && p.branch.rows == branch.rows && p.branch.shape == branch.shape
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
    shape: Option<Shape>,
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
            && self.shape.is_none_or(|shape| {
                !matches!(geom, FeatureGeom::Points(_)) && bounding_box(geom).is_some_and(|bbox| shape.holds(bbox))
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

/// The rule a feature matches.
enum Matched {
    /// The rule at this index into the layer's rules.
    Rule(usize),
    /// None of them, so the layer's own settings apply.
    NoRule,
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
    plan: &'a Plan,
    /// The index of the layer's table in the plan.
    table: usize,
    columns: &'a [ColumnDef],
    exprs: &'a [PlannedExpr],
    layer: &'a PlannedLayer,
    layer_index: usize,
    /// The known values of the table's columns, only those the column's kind can take.
    known: &'k [KnownValues],
    /// What is known from outside the plan about the table's columns.
    facts: &'k [ColumnFacts],
}

/// A part of a path's rows: the expression it comes from, and its condition.
type Part<'a> = (&'a str, Result<Condition, TranslateError>);

impl<'a> LayerPaths<'a, '_> {
    /// The layer's paths: its `where`, its rules, the choices in its expressions, the known values of the columns
    /// they read, every feature when none of these split the layer, and the shapes its geometries take.
    fn all(&self) -> Vec<Path<'a>> {
        let mut out = Vec::new();
        if let Some(filter) = self.layer.filter.map(|f| self.expr(f)) {
            self.filter(filter, &mut out);
        }
        self.rules(&mut out);
        self.expressions(&mut out);
        self.values(&mut out);
        if out.is_empty() {
            out.push(self.path("every feature".to_owned(), self.base(), self.test(Reach::Layer)));
        }
        self.shapes(&mut out);
        out
    }

    /// The rows `where` takes, those it does not, and those it drops for a `null` column, unless the known values
    /// decide it.
    fn filter(&self, filter: &'a PlannedExpr, out: &mut Vec<Path<'a>>) {
        if self.decided(filter.expr.ast()) {
            return;
        }
        let source = &filter.source;
        out.push(self.path(
            format!("kept by `where: {source}`"),
            self.base(),
            self.test(Reach::Layer),
        ));
        let mut parts = self.geometry();
        parts.push(self.part(filter, Polarity::Negated));
        let mut test = self.test(Reach::Geometry);
        test.checks.push((filter.expr.ast().clone(), Want::False));
        out.push(self.path(format!("dropped, `where: {source}` is false"), parts, test));
        for column in filter.expr.columns().filter(|c| self.may_be_null(c)) {
            let mut parts = self.geometry();
            parts.push((source.as_str(), Ok(column_is_null(column))));
            let mut test = self.test(Reach::Geometry);
            test.checks.push((property(column), Want::Null));
            let path = format!("dropped, `{column}` is null in `where: {source}`");
            out.push(self.path(path, parts, test));
        }
    }

    /// The first rule that matches takes the feature, so a rule's rows fail every earlier rule. The known values
    /// name the rule instead when they decide it and every earlier one.
    fn rules(&self, out: &mut Vec<Path<'a>>) {
        let mut decided = self.has_values();
        for (i, case) in self.layer.cases.iter().enumerate() {
            decided &= case.when.is_none_or(|when| self.decided(self.expr(when).expr.ast()));
            let path = match case.when {
                Some(when) => format!("rule {} `where: {}`", i + 1, self.expr(when).source),
                None => format!("rule {}, every other feature", i + 1),
            };
            if !decided {
                out.push(self.path(path, self.case_rows(Some(i)), self.test(Reach::Cases(vec![Some(i)]))));
            }
            if case.when.is_none() {
                return;
            }
        }
        if !self.layer.cases.is_empty() && !decided {
            out.push(self.path(
                "no rule matches".to_owned(),
                self.case_rows(None),
                self.test(Reach::Cases(vec![None])),
            ));
        }
    }

    /// Both sides of each `? :`, and each argument of a `coalesce` being the first that is not `null`, on the rows of
    /// the rules that evaluate the expression, unless the known values decide them.
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
            let field = self.field_of(index);
            let reach = self.reach_of(index);
            let case_decided = reach == Reach::Layer || self.cases_decided();
            let decided = |expr: &IdedExpr| case_decided && self.decided(expr);
            let base = || self.reached_rows(&reach);
            for choice in choices(planned.expr.ast()) {
                match choice {
                    Choice::Test { test, sides, root } => {
                        if decided(test) {
                            continue;
                        }
                        for (polarity, want, side, value) in [
                            (Polarity::AsWritten, Want::True, "true", sides[0]),
                            (Polarity::Negated, Want::False, "false", sides[1]),
                        ] {
                            let mut parts = base();
                            parts.push((source, self.translate(test, polarity)));
                            let mut check = self.test(reach.clone());
                            check.checks.push((test.clone(), want));
                            let path = if root {
                                format!("{field} {}, `{}` is {side}", Side(value), Cel(test))
                            } else {
                                format!("{field}, `{}` is {side}", Cel(test))
                            };
                            out.push(self.path(path, parts, check));
                        }
                        for column in read_columns(test).into_iter().filter(|c| self.may_be_null(c)) {
                            let mut parts = base();
                            parts.push((source, Ok(column_is_null(&column))));
                            let mut check = self.test(reach.clone());
                            check.checks.push((property(&column), Want::Null));
                            let path = format!("{field}, `{column}` is null in `{}`", Cel(test));
                            out.push(self.path(path, parts, check));
                        }
                    }
                    Choice::Coalesce { args, root } => {
                        for (k, arg) in args.iter().enumerate() {
                            if args[..=k].iter().all(decided) {
                                continue;
                            }
                            let mut parts = base();
                            parts.extend(args[..k].iter().map(|a| (source, self.is_null(a, Polarity::AsWritten))));
                            parts.push((source, self.is_null(arg, Polarity::Negated)));
                            let mut check = self.test(reach.clone());
                            check.checks.extend(args[..k].iter().map(|a| (a.clone(), Want::Null)));
                            check.checks.push((arg.clone(), Want::NotNull));
                            out.push(self.path(format!("{field} from `{}`", Cel(arg)), parts, check));
                        }
                        if args.iter().all(decided) {
                            continue;
                        }
                        let mut parts = base();
                        parts.extend(args.iter().map(|a| (source, self.is_null(a, Polarity::AsWritten))));
                        let mut check = self.test(reach.clone());
                        check.checks.extend(args.iter().map(|a| (a.clone(), Want::Null)));
                        let path = if root {
                            format!("{field} null, every `coalesce` argument is null")
                        } else {
                            format!("{field}, every `coalesce` argument is null")
                        };
                        out.push(self.path(path, parts, check));
                    }
                }
            }
        }
    }

    /// Each known value of a column the layer's expressions read, and `null` when the column may be `null`, with what
    /// the layer makes of a feature with that value wherever the known values decide it.
    fn values(&self, out: &mut Vec<Path<'a>>) {
        let read: Vec<String> = self
            .exprs
            .iter()
            .filter(|planned| planned.layer == self.layer_index)
            .flat_map(|planned| read_columns(planned.expr.ast()))
            .collect();
        for known in self.known.iter().filter(|k| read.contains(&k.column)) {
            let values = known.values.iter().map(Some);
            for value in values.chain(known.nullable.then_some(None)) {
                if let Some(path) = self.value_path(&known.column, value) {
                    out.push(path);
                }
            }
        }
    }

    /// The path of the features with `value` in `column`, `null` for `None`, or `None` when expressions never see
    /// `value` there.
    fn value_path(&self, column: &str, value: Option<&Literal>) -> Option<Path<'a>> {
        let kind = match value {
            Some(value) => Some(compared_as(self.columns, column, value)?),
            None => None,
        };
        let others = self.known.iter().filter(|k| k.column != column);
        let assigned: Vec<(&str, Prop<SourceStr>)> = (value.zip(kind.as_ref()))
            .map(|(value, kind)| (column, prop(value, kind.as_ref().ok().copied())))
            .into_iter()
            .chain(others.filter_map(|k| {
                let only = k.single()?;
                let kind = compared_as(self.columns, &k.column, only)?;
                Some((k.column.as_str(), prop(only, kind.as_ref().ok().copied())))
            }))
            .collect();
        let decides = |expr: &IdedExpr| self.decided_by(expr, column);
        let eval = |expr: &IdedExpr| self.eval_with(expr, &assigned);
        let filter = self.layer.filter.map(|f| self.expr(f));
        let filter_decided = filter.is_none_or(|f| decides(f.expr.ast()));
        let mut outcomes = Vec::new();
        let mut dropped = false;
        if let Some(f) = filter.filter(|_| filter_decided) {
            let source = &f.source;
            let (outcome, kept) = match eval(f.expr.ast()) {
                Ok(Value::Bool(true)) => (format!("kept by `where: {source}`"), true),
                Err(_) => (format!("dropped, `where: {source}` fails"), false),
                Ok(_) => (format!("dropped, `where: {source}` is false"), false),
            };
            dropped = !kept;
            outcomes.push(outcome);
        }
        if !dropped {
            let fields = match self.decided_case(&decides, &eval) {
                Some(Matched::Rule(i)) => {
                    outcomes.push(format!("rule {}", i + 1));
                    self.fields(self.case(Some(i)))
                }
                Some(Matched::NoRule) => {
                    if !self.layer.cases.is_empty() {
                        outcomes.push("no rule matches".to_owned());
                    }
                    self.fields(self.case(None))
                }
                None => self.shared_fields(),
            };
            for (field, index) in fields {
                let expr = self.exprs[index].expr.ast();
                if decides(expr) {
                    outcomes.push(format!("{field} {}", Outcome(eval(expr))));
                }
            }
        }
        let mut path = match value {
            Some(value) => format!("{column} {}", LiteralCel(value)),
            None => format!("{column} null"),
        };
        if !outcomes.is_empty() {
            path = format!("{path} -> {}", outcomes.join(", "));
        }
        let mut parts = if filter_decided { self.geometry() } else { self.base() };
        let mut test = self.test(if filter_decided { Reach::Geometry } else { Reach::Layer });
        let condition = if let Some((value, kind)) = value.zip(kind) {
            test.checks.push((equals(column, value), Want::True));
            self.equals(column, value, kind)
        } else {
            test.checks.push((property(column), Want::Null));
            self.no_keys().map(|()| column_is_null(column))
        };
        parts.push(("the column's known values", condition));
        Some(self.path(path, parts, test))
    }

    /// The rule a feature whose values `decides` decides and `eval` evaluates matches, or `None` when the values do
    /// not decide it.
    fn decided_case(
        &self,
        decides: &impl Fn(&IdedExpr) -> bool,
        eval: &impl Fn(&IdedExpr) -> Result<Value, ExecutionError>,
    ) -> Option<Matched> {
        for (i, case) in self.layer.cases.iter().enumerate() {
            let Some(when) = case.when else {
                return Some(Matched::Rule(i));
            };
            let when = self.expr(when).expr.ast();
            if !decides(when) {
                return None;
            }
            if matches!(eval(when), Ok(Value::Bool(true))) {
                return Some(Matched::Rule(i));
            }
        }
        Some(Matched::NoRule)
    }

    /// The expressions setting a field of `case`, each with the field's name, e.g. `attributes.class`.
    fn fields(&self, case: &Case) -> Vec<(String, usize)> {
        let mut out = Vec::new();
        for (name, zoom) in [("minzoom", case.minzoom), ("maxzoom", case.maxzoom)] {
            if let PlannedZoom::Expr(expr, _) = zoom {
                out.push((name.to_owned(), expr.index));
            }
        }
        for attr in case.attrs.computed.iter().chain(&case.attrs.banded) {
            if let PlannedValue::Expr(expr) = attr.value {
                let key = self.layer.keys.get(attr.key.index()).map_or("?", String::as_str);
                out.push((format!("attributes.{key}"), expr.index));
            }
        }
        if let PlannedId::Expr(expr) = self.layer.id {
            out.push(("id".to_owned(), expr.index));
        }
        out.extend(self.layer.sort_by.iter().map(|s| ("sort_by".to_owned(), s.index)));
        out
    }

    /// The fields every rule a feature can match sets with the same expression.
    fn shared_fields(&self) -> Vec<(String, usize)> {
        let mut cases = self.reachable().into_iter().map(|case| self.fields(self.case(case)));
        let first = cases.next().unwrap_or_default();
        let rest: Vec<_> = cases.collect();
        first
            .into_iter()
            .filter(|field| rest.iter().all(|fields| fields.contains(field)))
            .collect()
    }

    /// The field expression `index` sets, e.g. `minzoom` or `rule 2 attributes.class`.
    fn field_of(&self, index: usize) -> String {
        if let Some((name, _)) = self.fields(&self.layer.fallback).into_iter().find(|(_, i)| *i == index) {
            return name;
        }
        for (i, case) in self.layer.cases.iter().enumerate() {
            if let Some((name, _)) = self.fields(case).into_iter().find(|(_, e)| *e == index) {
                return format!("rule {} {name}", i + 1);
            }
        }
        self.exprs[index].source.clone()
    }

    /// Whether some column the layer reads has known values.
    fn has_values(&self) -> bool {
        self.exprs
            .iter()
            .filter(|planned| planned.layer == self.layer_index)
            .flat_map(|planned| read_columns(planned.expr.ast()))
            .any(|column| self.known.iter().any(|k| k.column == column))
    }

    /// Whether the known values decide the rule every feature matches.
    fn cases_decided(&self) -> bool {
        self.layer
            .cases
            .iter()
            .all(|case| case.when.is_none_or(|when| self.decided(self.expr(when).expr.ast())))
    }

    /// Whether the value of some column with known values decides `expr`.
    fn decided(&self, expr: &IdedExpr) -> bool {
        read_columns(expr)
            .iter()
            .any(|column| self.known.iter().any(|k| k.column == *column) && self.decided_by(expr, column))
    }

    /// Whether `expr` reads `column` and otherwise only columns with one known value, so a value of `column` decides
    /// it.
    fn decided_by(&self, expr: &IdedExpr, column: &str) -> bool {
        let read = read_columns(expr);
        read.iter().any(|c| c == column)
            && read
                .iter()
                .all(|c| c == column || self.known.iter().any(|k| k.column == *c && k.single().is_some()))
    }

    /// `expr` on a feature with only the properties `assigned`.
    fn eval_with(&self, expr: &IdedExpr, assigned: &[(&str, Prop<SourceStr>)]) -> Result<Value, ExecutionError> {
        let props = assigned
            .iter()
            .filter_map(|(column, value)| {
                let pos = self.columns.iter().position(|c| c.name() == *column)?;
                Some((key_id(pos), value.clone()))
            })
            .collect();
        let feature = SourceFeature {
            id: None,
            geometry: Geometry::Point(Coord { x: 0.0, y: 0.0 }),
            props,
        };
        self.plan.with_view(self.table, &feature, |view| eval_part(expr, view))
    }

    /// For the rules of each `min_size`, lines and polygons kept only at the layer's max zoom, those too small for
    /// every zoom, those shorter than a tile unit at the max zoom, and those crossing a tile edge there.
    fn shapes(&self, out: &mut Vec<Path<'a>>) {
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
            let mut shape = |path: String, shape: Shape| {
                let mut test = self.test(reach.clone());
                test.shape = Some(shape);
                let mut path = self.path(format!("{prefix}{path}"), parts.clone(), test);
                path.branch.shape = Some(shape);
                out.push(path);
            };
            let (min, max) = (*self.layer.zooms.start(), *self.layer.zooms.end());
            let at_max = min_size.at_max_zoom;
            let below = min_size.below_max_zoom;
            if min < max && at_max < 2.0 * below {
                let band = SizeBand {
                    zoom: max,
                    min: at_max,
                    max: 2.0 * below,
                };
                shape(
                    format!("kept only at z{max} (`min_size_at_maxzoom`)"),
                    Shape::Size(band),
                );
            }
            if at_max > 0.0 {
                let band = SizeBand {
                    zoom: max,
                    min: 0.0,
                    max: at_max,
                };
                shape(
                    "dropped, smaller than `min_size` at every zoom".to_owned(),
                    Shape::Size(band),
                );
            }
            let unit = 256.0 / f64::from(self.layer.render.grid.extent);
            if at_max < unit {
                let band = SizeBand {
                    zoom: max,
                    min: at_max,
                    max: unit,
                };
                shape(format!("shorter than one tile unit at z{max}"), Shape::Size(band));
            }
            shape(format!("crosses a z{max} tile edge"), Shape::CrossesTileEdge(max));
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
        self.no_keys()?;
        condition_of(expr, self.columns, polarity)
    }

    /// `column == value` compared as `kind`, which no source can check while a `jsonb` column may replace the columns
    /// it reads.
    fn equals(
        &self,
        column: &str,
        value: &Literal,
        kind: Result<ValueKind, TranslateError>,
    ) -> Result<Condition, TranslateError> {
        self.no_keys()?;
        let kind = kind?;
        Ok(Condition::Compare {
            left: Term::Column(column.to_owned()),
            op: CompareOp::Eq,
            right: Term::Literal(value.clone()),
            kind,
        })
    }

    /// An error when the table has a `jsonb` column whose keys may replace the columns a condition reads, so no source
    /// can check one.
    fn no_keys(&self) -> Result<(), TranslateError> {
        match self.columns.iter().find(|c| matches!(c, ColumnDef::Keys(_))) {
            Some(keys) => Err(TranslateError::KeysMayReplaceColumns(keys.name().to_owned())),
            None => Ok(()),
        }
    }

    /// A test on the features of `reach`, with no checks yet.
    fn test(&self, reach: Reach) -> Test<'a> {
        Test {
            layer: self.layer,
            reach,
            checks: Vec::new(),
            shape: None,
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
                shape: None,
            },
            test,
        }
    }

    /// The planned expression `filter` refers to.
    fn expr(&self, filter: ExprRef<Filter>) -> &'a PlannedExpr {
        &self.exprs[filter.index]
    }

    /// Whether `name` is a column read by value that nothing known rules out being `null`.
    fn may_be_null(&self, name: &str) -> bool {
        self.is_value(name) && !self.facts.iter().any(|f| f.column == name && f.not_null)
    }

    /// Whether `name` is a column read by value.
    fn is_value(&self, name: &str) -> bool {
        self.columns
            .iter()
            .any(|c| matches!(c, ColumnDef::Value(n, _) if n == name))
    }
}

/// The kind `column` of `columns` and `value` compare as, an error when the column's kind is not known, or `None` when
/// the column reads as values of another kind, e.g. a `numeric` column read as text, so expressions never see `value`
/// there.
fn compared_as(columns: &[ColumnDef], column: &str, value: &Literal) -> Option<Result<ValueKind, TranslateError>> {
    let kind = match value {
        Literal::Bool(_) => ValueKind::Bool,
        Literal::Int(_) | Literal::UInt(_) => ValueKind::Int,
        Literal::Float(_) => ValueKind::Float,
        Literal::String(_) => ValueKind::String,
    };
    let read_as = columns.iter().find_map(|c| match c {
        ColumnDef::Value(name, read_as) if name == column => Some(*read_as),
        ColumnDef::Value(..) | ColumnDef::Keys(_) => None,
    });
    match read_as {
        Some(Some(read_as)) => read_as.common(kind).map(Ok),
        Some(None) => Some(Err(TranslateError::UnknownColumnType(column.to_owned()))),
        None => Some(Ok(kind)),
    }
}

/// `value` as a source reads it from a column of `kind`, as itself when the kind is not known, saturating an unsigned
/// integer above `i64::MAX`, which no source reads.
fn prop(value: &Literal, kind: Option<ValueKind>) -> Prop<SourceStr> {
    match (value, kind) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a float column holds the nearest float to the number too"
        )]
        (Literal::Int(v), Some(ValueKind::Float)) => Prop::F64(*v as f64),
        #[expect(
            clippy::cast_precision_loss,
            reason = "a float column holds the nearest float to the number too"
        )]
        (Literal::UInt(v), Some(ValueKind::Float)) => Prop::F64(*v as f64),
        (Literal::Bool(v), _) => Prop::Bool(*v),
        (Literal::Int(v), _) => Prop::I64(*v),
        (Literal::UInt(v), _) => Prop::I64(i64::try_from(*v).unwrap_or(i64::MAX)),
        (Literal::Float(v), _) => Prop::F64(*v),
        (Literal::String(v), _) => Prop::Str(v.as_str().into()),
    }
}

/// A side of a `? :`: a literal as CEL writes it, anything else quoted, e.g. ``6`` or `` `lanes + 2` ``.
struct Side<'a>(&'a IdedExpr);

impl Display for Side<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.0.expr {
            Expr::Literal(_) => write!(f, "{}", Cel(self.0)),
            Expr::Unspecified
            | Expr::Call(_)
            | Expr::Comprehension(_)
            | Expr::Ident(_)
            | Expr::List(_)
            | Expr::Map(_)
            | Expr::Select(_)
            | Expr::Struct(_) => write!(f, "`{}`", Cel(self.0)),
        }
    }
}

/// What an expression evaluates to, as CEL writes it, or `fails`.
struct Outcome(Result<Value, ExecutionError>);

impl Display for Outcome {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Ok(Value::Null) => f.write_str("null"),
            Ok(Value::Bool(v)) => write!(f, "{v}"),
            Ok(Value::Int(v)) => write!(f, "{v}"),
            Ok(Value::UInt(v)) => write!(f, "{v}u"),
            Ok(Value::Float(v)) => write!(f, "{v:?}"),
            Ok(Value::String(v)) => write!(f, "'{}'", v.replace('\\', "\\\\").replace('\'', "\\'")),
            Ok(other) => write!(f, "{other:?}"),
            Err(_) => f.write_str("fails"),
        }
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

/// A place where an expression picks one of several values, `root` when it is the whole expression.
enum Choice<'e> {
    /// The test of a `? :`, and the values it picks between.
    Test {
        test: &'e IdedExpr,
        sides: [&'e IdedExpr; 2],
        root: bool,
    },
    /// The arguments of `coalesce`.
    Coalesce { args: &'e [IdedExpr], root: bool },
}

/// The choices in `expr`, outermost first.
fn choices(expr: &IdedExpr) -> Vec<Choice<'_>> {
    let mut out = Vec::new();
    walk(expr, &mut |e| {
        if let Expr::Call(call) = &e.expr {
            let root = std::ptr::eq(e, expr);
            match (call.func_name.as_str(), call.target.as_deref(), call.args.as_slice()) {
                (operators::CONDITIONAL, None, [test, yes, no]) => out.push(Choice::Test {
                    test,
                    sides: [yes, no],
                    root,
                }),
                ("coalesce", None, args) if args.len() > 1 => out.push(Choice::Coalesce { args, root }),
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
