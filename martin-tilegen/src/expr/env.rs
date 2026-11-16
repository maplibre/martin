use std::sync::{Arc, LazyLock};

use cel::common::types::{CelBool, CelDouble, CelString, Kind};
use cel::common::value::CowVal;
use cel::{Context, Env, ExecutionError, FunctionContext, extensions};

use super::area::AreaUnit;
use super::{NULL, text};

const COALESCE: &str = "coalesce";
/// The largest of its arguments, or of the one list it is given.
const MAX: &str = "max";
/// The smallest of its arguments, or of the one list it is given.
const MIN: &str = "min";
/// The area of the feature's whole geometry in the unit it is given, e.g. `area('m2')`.
pub(crate) const AREA: &str = "area";
/// Whether a string holds only non-letters and Latin letters, e.g. `name.isLatin()`.
const IS_LATIN: &str = "isLatin";
/// A string without its Latin letters, e.g. `name.removeLatin()`.
const REMOVE_LATIN: &str = "removeLatin";
/// A string with each match of a pattern replaced, e.g. `regex.replace(ref, '[^0-9]', '')`.
const REGEX_REPLACE: &str = "regex.replace";
/// The functions Martin adds to CEL's standard library.
const ADDED: [&str; 7] = [COALESCE, MAX, MIN, AREA, IS_LATIN, REMOVE_LATIN, REGEX_REPLACE];

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
        root.add_function(MIN, cel::functions::min)
            .expect("the standard library has no min");
        root.add_function(AREA, Box::new(area) as Function)
            .expect("the standard library has no area");
        root.add_function(IS_LATIN, Box::new(is_latin) as Function)
            .expect("the standard library has no isLatin");
        root.add_function(REMOVE_LATIN, Box::new(remove_latin) as Function)
            .expect("the standard library has no removeLatin");
        root.add_function(REGEX_REPLACE, Box::new(regex_replace) as Function)
            .expect("the standard library has no regex.replace");
        Self { env, root }
    }

    pub(super) fn is_function(&self, name: &str) -> bool {
        ADDED.contains(&name)
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

/// `isLatin()`: whether the string holds only non-letters and Latin letters, `false` for `null`.
fn is_latin<'c, 'v>(ftx: &mut FunctionContext<'c, 'v>) -> Result<CowVal<'c, 'v>, ExecutionError> {
    let latin = text_operand(ftx)?.is_some_and(text::is_latin);
    Ok(CowVal::owned(CelBool::from(latin)))
}

/// `removeLatin()`: the string without its Latin letters, `null` when only blanks are left.
fn remove_latin<'c, 'v>(ftx: &mut FunctionContext<'c, 'v>) -> Result<CowVal<'c, 'v>, ExecutionError> {
    Ok(match text_operand(ftx)?.and_then(text::remove_latin) {
        Some(rest) => CowVal::owned(CelString::from(rest)),
        None => CowVal::Borrowed(&NULL),
    })
}

/// `regex.replace(text, pattern, replacement)`: `text` with each match replaced, `null` for a `null` text.
fn regex_replace<'c, 'v>(ftx: &mut FunctionContext<'c, 'v>) -> Result<CowVal<'c, 'v>, ExecutionError> {
    let (None, [text, pattern, replacement]) = (&ftx.this, ftx.args.as_slice()) else {
        return Err(ExecutionError::function_error(
            REGEX_REPLACE,
            "takes a text, a pattern and a replacement, e.g. `regex.replace(ref, '[^0-9]', '')`",
        ));
    };
    if text.get_type().kind() == Kind::NullType {
        return Ok(CowVal::Borrowed(&NULL));
    }
    let replaced = text::replace_all(string(text)?, string(pattern)?, string(replacement)?)
        .map_err(|message| ExecutionError::function_error(REGEX_REPLACE, message))?;
    Ok(CowVal::owned(CelString::from(replaced)))
}

/// The one string a text function reads, from its target or its only argument, `None` for `null`.
fn text_operand<'f>(ftx: &'f FunctionContext<'_, '_>) -> Result<Option<&'f str>, ExecutionError> {
    let ((Some(value), []) | (None, [value])) = (&ftx.this, ftx.args.as_slice()) else {
        return Err(ftx.error("takes one string, e.g. `name.isLatin()`"));
    };
    if value.get_type().kind() == Kind::NullType {
        return Ok(None);
    }
    string(value).map(Some)
}

/// The string `value` holds, else an error naming its type.
fn string<'a>(value: &'a CowVal<'_, '_>) -> Result<&'a str, ExecutionError> {
    value
        .downcast_ref::<CelString>()
        .map(CelString::inner)
        .ok_or_else(|| ExecutionError::UnexpectedType {
            got: value.get_type().name().to_owned(),
            want: "string".to_owned(),
        })
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
    #[case::of_a_list("min(ref.split(';').map(r, size(r)))", Value::Int(4))]
    #[case::of_arguments("min(size(ref), 3)", Value::Int(3))]
    fn min_returns_the_smallest(#[case] source: &str, #[case] expected: Value) {
        let columns = ["ref"];
        let expr = CompiledExpr::compile(source, &columns, false).unwrap();
        let keys = ExprKeys::columns(&columns);
        let props = vec![(KeyId::new(0), Prop::Str("B 39;B 39A".to_owned()))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), expected);
    }

    #[rstest]
    #[case::member("housenumber.matches('^[0-9]+$')", Value::Bool(false))]
    #[case::global("matches(housenumber, '^[0-9a-z;]+$')", Value::Bool(true))]
    #[case::replace("regex.replace(housenumber, '[^0-9;]', '')", Value::from("5;7"))]
    #[case::replace_groups(r"regex.replace(housenumber, '([0-9]+)([a-z])', '\\2\\1')", Value::from("a5;b7"))]
    #[case::replace_whole_match(r"regex.replace(housenumber, '[0-9]', '<\\0>')", Value::from("<5>a;<7>b"))]
    #[case::replace_dollar("regex.replace(housenumber, ';', '$1')", Value::from("5a$17b"))]
    #[case::replace_null("regex.replace(street, ';', '')", Value::Null)]
    fn regular_expressions_match_and_replace(#[case] source: &str, #[case] expected: Value) {
        let columns = ["housenumber", "street"];
        let expr = CompiledExpr::compile(source, &columns, false).unwrap();
        let keys = ExprKeys::columns(&columns);
        let props = vec![(KeyId::new(0), Prop::Str("5a;7b".to_owned()))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), expected);
    }

    #[test]
    fn regex_replace_with_an_invalid_pattern_fails() {
        let expr = CompiledExpr::compile("regex.replace(ref, '(B', '')", &["ref"], false).unwrap();
        let keys = ExprKeys::columns(&["ref"]);
        let props = vec![(KeyId::new(0), Prop::Str("B 39".to_owned()))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        insta::assert_snapshot!(expr.eval(&view).unwrap_err(), @"
        Error executing function 'regex.replace': '(B' not a valid regex:
        regex parse error:
            (B
            ^
        error: unclosed group
        ");
    }

    #[test]
    fn regex_replace_with_an_unknown_escape_fails() {
        let expr = CompiledExpr::compile(r"regex.replace(ref, 'B', '\\n')", &["ref"], false).unwrap();
        let keys = ExprKeys::columns(&["ref"]);
        let props = vec![(KeyId::new(0), Prop::Str("B 39".to_owned()))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        insta::assert_snapshot!(expr.eval(&view).unwrap_err(), @r"Error executing function 'regex.replace': invalid replacement escape `\n`");
    }

    #[rstest]
    #[case::ascii("abc", true)]
    #[case::digit_and_punctuation("5!", true)]
    #[case::digit_and_tilde("5~", true)]
    #[case::accent("é", true)]
    #[case::accents("éś", true)]
    #[case::latin_extended_b("ɏə", true)]
    #[case::ipa("ɐ", true)]
    #[case::greek_extended("ᵿἀ", false)]
    #[case::latin_extended_additional("Ḁỿ", true)]
    #[case::greek_letter_after_a_modifier("\u{2ff}\u{370}", false)]
    #[case::combining_mark("\u{30}\u{36f}", true)]
    #[case::japanese("日本", false)]
    #[case::mixed("abc本123", false)]
    #[case::empty("", false)]
    fn is_latin_matches_planetiler(#[case] name: &str, #[case] expected: bool) {
        let expr = CompiledExpr::compile("name.isLatin()", &["name"], false).unwrap();
        let keys = ExprKeys::columns(&["name"]);
        let props = vec![(KeyId::new(0), Prop::Str(name.to_owned()))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), Value::Bool(expected));
    }

    #[test]
    fn is_latin_of_null_is_false() {
        let expr = CompiledExpr::compile("name.isLatin()", &["name"], false).unwrap();
        let keys = ExprKeys::columns(&["name"]);
        let props: Vec<(KeyId, Prop)> = vec![];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), Value::Bool(false));
    }

    #[rstest]
    #[case::latin_and_a_plus(Some("abcaāíìś+"), Value::from("+"))]
    #[case::only_latin(Some("abcaāíìś"), Value::Null)]
    #[case::nonlatin(Some("日本"), Value::from("日本"))]
    #[case::mixed(Some("abca日āíìś+"), Value::from("日+"))]
    #[case::parenthesized_latin(Some("(abc)"), Value::Null)]
    #[case::latin_in_parens(Some("日本 (Japan)"), Value::from("日本"))]
    #[case::latin_in_brackets(Some("日本 [Japan - Nippon]"), Value::from("日本"))]
    #[case::latin_around(
        Some("Japan - Nippon (Japan) - Japan - 日本 - Japan - Nippon (Japan)"),
        Value::from("日本")
    )]
    #[case::trailing_symbols(Some("Japan - 日本~+"), Value::from("日本~+"))]
    #[case::slashes(Some("Japan / 日本 / Japan"), Value::from("日本"))]
    #[case::null(None, Value::Null)]
    fn remove_latin_matches_planetiler(#[case] name: Option<&str>, #[case] expected: Value) {
        let expr = CompiledExpr::compile("name.removeLatin()", &["name"], false).unwrap();
        let keys = ExprKeys::columns(&["name"]);
        let props: Vec<(KeyId, Prop)> = name
            .map(|n| (KeyId::new(0), Prop::Str(n.to_owned())))
            .into_iter()
            .collect();
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), expected);
    }

    #[rstest]
    #[case::latin_name(&[Some("Berlin"), Some("Berlin EN"), None], Value::from("Berlin"))]
    #[case::english_name(&[Some("東京"), Some("Tokyo"), Some("Tōkyō")], Value::from("Tokyo"))]
    #[case::international_name(&[Some("東京"), Some("東京"), Some("Tōkyō")], Value::from("Tōkyō"))]
    #[case::no_latin_name(&[Some("ア"), None, Some("غ")], Value::Null)]
    fn name_latin_takes_the_first_latin_name(#[case] names: &[Option<&str>], #[case] expected: Value) {
        let columns = ["name", "name:en", "int_name"];
        let expr = CompiledExpr::compile(
            "name.isLatin() ? name : feature['name:en'].isLatin() ? feature['name:en'] : int_name.isLatin() ? int_name : null",
            &columns,
            false,
        )
        .unwrap();
        let keys = ExprKeys::columns(&columns);
        let props: Vec<(KeyId, Prop)> = (0..)
            .zip(names)
            .filter_map(|(i, name)| Some((KeyId::new(i), Prop::Str((*name)?.to_owned()))))
            .collect();
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
    #[case::z0_pixels("area('z0 px2')", 0.505_685_430_744_375_1)]
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
