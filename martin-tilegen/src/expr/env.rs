use std::sync::{Arc, LazyLock};

use cel::common::types::{CelDouble, CelString, Kind};
use cel::common::value::CowVal;
use cel::{Context, Env, ExecutionError, FunctionContext, extensions};

use super::NULL;
use super::area::AreaUnit;

const COALESCE: &str = "coalesce";
/// The largest of its arguments, or of the one list it is given.
const MAX: &str = "max";
/// The area of the feature's whole geometry in the unit it is given, e.g. `area('m2')`.
pub(crate) const AREA: &str = "area";

type Function =
    Box<dyn for<'c, 'v> Fn(&mut FunctionContext<'c, 'v>) -> Result<CowVal<'c, 'v>, ExecutionError> + Send + Sync>;

pub(super) static SHARED: LazyLock<Shared> = LazyLock::new(Shared::new);

pub(super) struct Shared {
    pub(super) env: Arc<Env>,
    pub(super) root: Context<'static, 'static>,
}

impl Shared {
    fn new() -> Self {
        let mut env = Env::stdlib();
        env.add_extension(extensions::strings)
            .expect("the strings extension registers on the standard library");
        let env = Arc::new(env);
        let mut root = Context::with_env(Arc::clone(&env));
        root.add_function(COALESCE, Box::new(coalesce) as Function)
            .expect("the standard library has no coalesce");
        root.add_function(MAX, cel::functions::max)
            .expect("the standard library has no max");
        root.add_function(AREA, Box::new(area) as Function)
            .expect("the standard library has no area");
        Self { env, root }
    }

    pub(super) fn is_function(&self, name: &str) -> bool {
        name == COALESCE
            || name == MAX
            || name == AREA
            || Context::with_env(Arc::clone(&self.env))
                .add_function(name, Box::new(coalesce) as Function)
                .is_err()
    }

    pub(super) fn is_type(&self, name: &str) -> bool {
        self.env.types().find_type(name).is_some()
    }
}

#[expect(clippy::unnecessary_wraps, reason = "signature fixed by cel's function registry")]
fn coalesce<'c, 'v>(ftx: &mut FunctionContext<'c, 'v>) -> Result<CowVal<'c, 'v>, ExecutionError> {
    let args = std::mem::take(&mut ftx.args);
    Ok(ftx
        .this
        .take()
        .into_iter()
        .chain(args)
        .find(|v| v.get_type().kind() != Kind::NullType)
        .unwrap_or(CowVal::Borrowed(&NULL)))
}

/// `area(unit)`: the feature's area in `unit`, from the whole geometry its view measures.
fn area<'c, 'v>(ftx: &mut FunctionContext<'c, 'v>) -> Result<CowVal<'c, 'v>, ExecutionError> {
    let (None, [unit]) = (&ftx.this, ftx.args.as_slice()) else {
        return Err(ftx.error("takes one unit and no target, e.g. `area('m2')`"));
    };
    let label = unit
        .downcast_ref::<CelString>()
        .ok_or_else(|| ExecutionError::UnexpectedType {
            got: unit.get_type().name().to_owned(),
            want: "string".to_owned(),
        })?;
    let unit =
        AreaUnit::parse(label.inner()).ok_or_else(|| ftx.error(format!("unknown area unit `{}`", label.inner())))?;
    let base = ftx
        .ptx
        .get_variable(unit.base.variable())
        .ok_or_else(|| ftx.error("has no geometry to measure here"))?;
    let base = base
        .downcast_ref::<CelDouble>()
        .ok_or_else(|| ftx.error("measured something other than a double"))?;
    Ok(CowVal::owned(CelDouble::from(unit.of(*base.inner()))))
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;
    use cel::Value;
    use geo_types::{Coord, LineString, Polygon};
    use rstest::rstest;

    use crate::FeatureGeom;
    use crate::expr::{Areas, CompiledExpr, ExprKeys, PropSlots};
    use crate::project::from_lonlat;
    use crate::props::{KeyId, Prop};

    #[rstest]
    #[case::first_present("coalesce(feature['name:en'], name)", Value::from("Munich"))]
    #[case::skips_null("coalesce(feature['name:de'], name)", Value::from("München"))]
    #[case::all_null("coalesce(feature['name:de'], null)", Value::Null)]
    #[case::literal_fallback("coalesce(population, 0)", Value::Int(0))]
    fn coalesce_returns_the_first_non_null(#[case] source: &str, #[case] expected: Value) {
        let columns = ["name", "name:en", "name:de", "population"];
        let expr = CompiledExpr::compile(source, &columns, false).unwrap();
        let keys = ExprKeys::columns(&columns);
        let props = vec![
            (KeyId::new(0), Prop::Str("München".to_owned())),
            (KeyId::new(1), Prop::Str("Munich".to_owned())),
        ];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), expected);
    }

    #[rstest]
    #[case::of_a_list("max(ref.split(';').map(r, size(r)))", Value::Int(5))]
    #[case::of_arguments("max(size(ref), 3)", Value::Int(10))]
    fn max_returns_the_largest(#[case] source: &str, #[case] expected: Value) {
        let columns = ["ref"];
        let expr = CompiledExpr::compile(source, &columns, false).unwrap();
        let keys = ExprKeys::columns(&columns);
        let props = vec![(KeyId::new(0), Prop::Str("B 39;B 39A".to_owned()))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), expected);
    }

    #[rstest]
    #[case::square_meters("area('m2')", 12_364_188_935.608_72)]
    #[case::square_kilometers("area('km2')", 12_364.188_935_608_72)]
    #[case::hectares("area('ha')", 1_236_418.893_560_872)]
    #[case::spelled_out("area('Square Metres')", 12_364_188_935.608_72)]
    #[case::square_feet("area('sq ft')", 133_087_022_078.165_85)]
    #[case::z0_tiles("area('z0 tiles')", 7.716_147_319_707_872e-6)]
    #[case::z0_pixels_as_planetiler_divides("area('z0 px2')", 1.177_390_643_265_971_7e-10)]
    fn area_measures_a_polygon_as_planetiler_does(#[case] source: &str, #[case] expected: f64) {
        let ring = [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5), (-0.5, -0.5)];
        let polygons = [Polygon::new(
            ring.into_iter().map(|(x, y)| from_lonlat(Coord { x, y })).collect(),
            vec![],
        )];
        let expr = CompiledExpr::compile(source, &["name"], false).unwrap();
        let keys = ExprKeys::columns(&["name"]);
        let props: Vec<(KeyId, Prop)> = vec![];
        let areas = Areas::new(FeatureGeom::Polygons(&polygons));
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props).with_areas(&areas);
        let Value::Float(area) = expr.eval(&view).unwrap() else {
            panic!("area is not a double");
        };
        assert_relative_eq!(area, expected, max_relative = 1e-12);
    }

    #[test]
    fn area_leaves_out_the_holes() {
        let outer = [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5), (-0.5, -0.5)];
        let hole = [
            (-0.25, -0.25),
            (-0.25, 0.25),
            (0.25, 0.25),
            (0.25, -0.25),
            (-0.25, -0.25),
        ];
        let ring = |coords: [(f64, f64); 5]| -> LineString {
            coords.into_iter().map(|(x, y)| from_lonlat(Coord { x, y })).collect()
        };
        let polygons = [Polygon::new(ring(outer), vec![ring(hole)])];
        let expr = CompiledExpr::compile("area('m2')", &["name"], false).unwrap();
        let keys = ExprKeys::columns(&["name"]);
        let props: Vec<(KeyId, Prop)> = vec![];
        let areas = Areas::new(FeatureGeom::Polygons(&polygons));
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props).with_areas(&areas);
        let Value::Float(area) = expr.eval(&view).unwrap() else {
            panic!("area is not a double");
        };
        assert_relative_eq!(area, 9_273_112_276.884_596, max_relative = 1e-12);
    }

    #[test]
    fn area_of_a_line_is_zero() {
        let lines = [LineString::from(vec![
            from_lonlat(Coord { x: -0.5, y: -0.5 }),
            from_lonlat(Coord { x: 0.5, y: -0.5 }),
            from_lonlat(Coord { x: 0.5, y: 0.5 }),
            from_lonlat(Coord { x: -0.5, y: -0.5 }),
        ])];
        let expr = CompiledExpr::compile("area('m2')", &["name"], false).unwrap();
        let keys = ExprKeys::columns(&["name"]);
        let props: Vec<(KeyId, Prop)> = vec![];
        let areas = Areas::new(FeatureGeom::Lines(&lines));
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props).with_areas(&areas);
        assert_eq!(expr.eval(&view).unwrap(), Value::Float(0.0));
    }

    #[test]
    fn area_without_a_geometry_fails() {
        let expr = CompiledExpr::compile("area('m2')", &["name"], false).unwrap();
        let keys = ExprKeys::columns(&["name"]);
        let props: Vec<(KeyId, Prop)> = vec![];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        insta::assert_snapshot!(expr.eval(&view).unwrap_err(), @"Error executing function 'area': has no geometry to measure here");
    }

    #[test]
    fn area_of_a_unit_from_a_property_is_checked_when_evaluated() {
        let polygons = [Polygon::new(
            LineString::from(vec![(0.5, 0.5), (0.6, 0.5), (0.6, 0.6), (0.5, 0.5)]),
            vec![],
        )];
        let expr = CompiledExpr::compile("area(unit)", &["unit"], false).unwrap();
        let keys = ExprKeys::columns(&["unit"]);
        let props = vec![(KeyId::new(0), Prop::Str("furlongs".to_owned()))];
        let areas = Areas::new(FeatureGeom::Polygons(&polygons));
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props).with_areas(&areas);
        insta::assert_snapshot!(expr.eval(&view).unwrap_err(), @"Error executing function 'area': unknown area unit `furlongs`");
    }
}
