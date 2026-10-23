//! The `PostgreSQL` side of `martin test`: the paths through the layers of a table source, with the SQL selecting
//! the rows on each.

use std::fmt::{self, Display, Formatter};

use martin_tile_utils::EARTH_CIRCUMFERENCE;
use martin_tilegen::TileGenError;
use martin_tilegen::plan::{Branch, ColumnValues, Condition, GeometryType, Literal, Plan, Shape, SizeBand};
use postgres_protocol::escape::escape_identifier;

use crate::config::file::postgres::TableInfo;
use crate::config::file::postgres::resolver::condition_sql;
use crate::generate::GenerateResult;
use crate::generate::layers::{LowerOptions, lower_table};
use crate::generate::postgres::ScanTable;

/// Every path through the layers of `info` at zooms 0 to `max_zoom`, without shapes for a source of points.
/// The values the source's `filter` allows a column give a path each to a layer reading that column.
///
/// # Errors
///
/// If the source cannot be planned.
pub fn paths(id: &str, info: &TableInfo, max_zoom: u8) -> GenerateResult<Vec<Branch>> {
    let Some(table) = lower(id, info.clone(), max_zoom)? else {
        return Ok(Vec::new());
    };
    let plan = Plan::new(vec![table.def]).map_err(TileGenError::from)?;
    let points = table_geometry(info) == Some(GeometryType::Point);
    Ok(plan
        .branches(0, &filter_values(info))?
        .into_iter()
        .filter(|branch| !(points && branch.shape.is_some()))
        .collect())
}

/// The values of the columns the `filter` of `info` allows only some of, from the `IN` lists and `=` it ANDs.
fn filter_values(info: &TableInfo) -> Vec<ColumnValues> {
    let Some(expr) = info.filter.as_deref().and_then(|f| cql2::parse_text(f).ok()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_values(&expr, &mut out);
    out
}

/// The column values `expr` and the conditions it ANDs allow.
fn collect_values(expr: &cql2::Expr, out: &mut Vec<ColumnValues>) {
    let cql2::Expr::Operation { op, args } = expr else {
        return;
    };
    let op = op.to_ascii_lowercase();
    if op == "and" {
        for arg in args {
            collect_values(arg, out);
        }
        return;
    }
    let [column, value] = args.as_slice() else {
        return;
    };
    let cql2::Expr::Property { property } = &**column else {
        return;
    };
    let values = if op == "in"
        && let cql2::Expr::Array(items) = &**value
    {
        items.iter().map(|item| literal(item)).collect()
    } else if op == "=" {
        literal(value).map(|value| vec![value])
    } else {
        None
    };
    if let Some(values) = values {
        out.push(ColumnValues {
            column: property.clone(),
            values,
        });
    }
}

/// A CQL2 literal, with whole numbers as integers.
fn literal(expr: &cql2::Expr) -> Option<Literal> {
    if let cql2::Expr::Literal(v) = expr {
        Some(Literal::String(v.clone()))
    } else if let cql2::Expr::Bool(v) = expr {
        Some(Literal::Bool(*v))
    } else if let cql2::Expr::Float(v) = expr {
        if v.fract() == 0.0 && v.abs() < 9.0e15 {
            #[expect(clippy::cast_possible_truncation, reason = "a whole number in range of i64")]
            return Some(Literal::Int(*v as i64));
        }
        Some(Literal::Float(*v))
    } else {
        None
    }
}

/// The geometry type of every row of `info`, when its column declares a single one.
fn table_geometry(info: &TableInfo) -> Option<GeometryType> {
    let declared = info.geometry_type.as_deref()?.trim().to_ascii_uppercase();
    let base = declared.trim_end_matches(['Z', 'M']);
    match base.strip_prefix("MULTI").unwrap_or(base) {
        "POINT" => Some(GeometryType::Point),
        "LINESTRING" => Some(GeometryType::Line),
        "POLYGON" => Some(GeometryType::Polygon),
        _ => None,
    }
}

/// Lowers `info` for zooms 0 to `max_zoom`.
fn lower(id: &str, info: TableInfo, max_zoom: u8) -> GenerateResult<Option<ScanTable>> {
    lower_table(
        id,
        info,
        &LowerOptions {
            zooms: 0..=max_zoom,
            bounds: None,
        },
    )
}

/// The SQL condition on the rows of a path.
pub struct RowsSql<'a>(pub &'a TableInfo, pub &'a Condition, pub Option<Shape>);

impl Display for RowsSql<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let Self(info, condition, shape) = *self;
        let condition = condition_sql(info, condition);
        match shape {
            Some(shape) => write!(f, "({condition}) AND {}", ShapeSql(info, shape)),
            None => f.write_str(&condition),
        }
    }
}

/// The SQL condition on the geometries of a shape, without a subquery, which would keep `PostgreSQL` from pushing
/// it below the joins of a view.
pub struct ShapeSql<'a>(pub &'a TableInfo, pub Shape);

impl Display for ShapeSql<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let Self(info, shape) = *self;
        let geom = escape_identifier(&info.geometry_column);
        let side = |side: &str| format!("ST_{side}(ST_Transform({geom}::geometry, 3857)::box2d)");
        let tile = |zoom: u8| EARTH_CIRCUMFERENCE / f64::from(1u32 << zoom);
        match shape {
            Shape::Size(SizeBand { zoom, min, max }) => {
                let meters = |pixels: f64| pixels * tile(zoom) / 256.0;
                write!(
                    f,
                    "numrange({}, {}) @> greatest({} - {}, {} - {})::numeric",
                    meters(min),
                    meters(max),
                    side("XMax"),
                    side("XMin"),
                    side("YMax"),
                    side("YMin")
                )
            }
            Shape::CrossesTileEdge(zoom) => {
                // Tile rows count from the north edge, as in the renderer's unit square.
                let (half, tile) = (EARTH_CIRCUMFERENCE / 2.0, tile(zoom));
                let column = |name: &str| format!("floor(({} + {half}) / {tile})", side(name));
                let row = |name: &str| format!("floor(({half} - {}) / {tile})", side(name));
                write!(
                    f,
                    "({} <> {} OR {} <> {})",
                    column("XMin"),
                    column("XMax"),
                    row("YMin"),
                    row("YMax")
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    fn listed(yaml: &str) -> String {
        let info: TableInfo = serde_saphyr::from_str(yaml).expect("parses");
        paths("source", &info, 14)
            .expect("plans")
            .iter()
            .map(|branch| {
                let sql = match &branch.rows {
                    Ok(condition) => RowsSql(&info, condition, branch.shape).to_string(),
                    Err(err) => format!("cannot query: {}", err.reason),
                };
                format!("{branch}\n    {sql}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn each_rule_takes_the_rows_no_earlier_rule_takes() {
        insta::assert_snapshot!(listed(indoc! {"
            schema: public
            table: roads
            srid: 4326
            geometry_column: geom
            geometry_type: LINESTRING
            properties:
              class: text
            layers:
              roads:
                rules:
                  - where: \"class == 'motorway'\"
                    minzoom: 4
                  - where: \"class == 'path'\"
                    minzoom: 12
        "}), @r#"
        roads: rule 1 `where: class == 'motorway'`
            "class" = 'motorway'
        roads: rule 2 `where: class == 'path'`
            "class" IS DISTINCT FROM 'motorway' AND "class" = 'path'
        roads: no rule matches
            "class" IS DISTINCT FROM 'motorway' AND "class" IS DISTINCT FROM 'path'
        roads: kept only at z14 (`min_size_at_maxzoom`)
            (ST_GeometryType("geom"::geometry) IN ('ST_LineString', 'ST_MultiLineString') OR ST_GeometryType("geom"::geometry) IN ('ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle')) AND numrange(0.5971642834779397, 19.10925707129407) @> greatest(ST_XMax(ST_Transform("geom"::geometry, 3857)::box2d) - ST_XMin(ST_Transform("geom"::geometry, 3857)::box2d), ST_YMax(ST_Transform("geom"::geometry, 3857)::box2d) - ST_YMin(ST_Transform("geom"::geometry, 3857)::box2d))::numeric
        roads: dropped, smaller than `min_size` at every zoom
            (ST_GeometryType("geom"::geometry) IN ('ST_LineString', 'ST_MultiLineString') OR ST_GeometryType("geom"::geometry) IN ('ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle')) AND numrange(0, 0.5971642834779397) @> greatest(ST_XMax(ST_Transform("geom"::geometry, 3857)::box2d) - ST_XMin(ST_Transform("geom"::geometry, 3857)::box2d), ST_YMax(ST_Transform("geom"::geometry, 3857)::box2d) - ST_YMin(ST_Transform("geom"::geometry, 3857)::box2d))::numeric
        roads: crosses a z14 tile edge
            (ST_GeometryType("geom"::geometry) IN ('ST_LineString', 'ST_MultiLineString') OR ST_GeometryType("geom"::geometry) IN ('ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle')) AND (floor((ST_XMin(ST_Transform("geom"::geometry, 3857)::box2d) + 20037508.34278925) / 2445.984905125641) <> floor((ST_XMax(ST_Transform("geom"::geometry, 3857)::box2d) + 20037508.34278925) / 2445.984905125641) OR floor((20037508.34278925 - ST_YMin(ST_Transform("geom"::geometry, 3857)::box2d)) / 2445.984905125641) <> floor((20037508.34278925 - ST_YMax(ST_Transform("geom"::geometry, 3857)::box2d)) / 2445.984905125641))
        "#);
    }

    #[test]
    fn lines_without_min_size_take_a_path_shorter_than_one_tile_unit() {
        insta::assert_snapshot!(listed(indoc! {"
            schema: public
            table: segments
            srid: 4326
            geometry_column: geom
            geometry_type: LINESTRING
            extent: 4096
            clip_geom: false
            properties:
              id: int8
            layers:
              segments:
                min_size: 0
        "}), @r#"
        segments: every feature
            TRUE
        segments: shorter than one tile unit at z14
            (ST_GeometryType("geom"::geometry) IN ('ST_LineString', 'ST_MultiLineString') OR ST_GeometryType("geom"::geometry) IN ('ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle')) AND numrange(0, 0.5971642834779397) @> greatest(ST_XMax(ST_Transform("geom"::geometry, 3857)::box2d) - ST_XMin(ST_Transform("geom"::geometry, 3857)::box2d), ST_YMax(ST_Transform("geom"::geometry, 3857)::box2d) - ST_YMin(ST_Transform("geom"::geometry, 3857)::box2d))::numeric
        segments: crosses a z14 tile edge
            (ST_GeometryType("geom"::geometry) IN ('ST_LineString', 'ST_MultiLineString') OR ST_GeometryType("geom"::geometry) IN ('ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle')) AND (floor((ST_XMin(ST_Transform("geom"::geometry, 3857)::box2d) + 20037508.34278925) / 2445.984905125641) <> floor((ST_XMax(ST_Transform("geom"::geometry, 3857)::box2d) + 20037508.34278925) / 2445.984905125641) OR floor((20037508.34278925 - ST_YMin(ST_Transform("geom"::geometry, 3857)::box2d)) / 2445.984905125641) <> floor((20037508.34278925 - ST_YMax(ST_Transform("geom"::geometry, 3857)::box2d)) / 2445.984905125641))
        "#);
    }

    #[test]
    fn a_table_of_points_has_no_shape_path_even_where_the_layer_takes_any_geometry() {
        insta::assert_snapshot!(listed(indoc! {"
            schema: public
            table: stops
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            properties:
              id: int8
            layers:
              stops: {}
        "}), @"
        stops: every feature
            TRUE
        ");
    }

    #[test]
    fn a_where_is_taken_not_taken_or_null() {
        insta::assert_snapshot!(listed(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            properties:
              population: int8
            layers:
              places:
                geometry: point
                where: population > 1000
        "}), @r#"
        places: kept by `where: population > 1000`
            ST_GeometryType("geom"::geometry) IN ('ST_Point', 'ST_MultiPoint') AND "population" > 1000
        places: dropped, `where: population > 1000` is false
            ST_GeometryType("geom"::geometry) IN ('ST_Point', 'ST_MultiPoint') AND "population" <= 1000
        places: dropped, `population` is null in `where: population > 1000`
            ST_GeometryType("geom"::geometry) IN ('ST_Point', 'ST_MultiPoint') AND "population" IS NULL
        "#);
    }

    #[test]
    fn each_side_of_a_choice_and_each_coalesce_argument_is_a_path() {
        insta::assert_snapshot!(listed(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            properties:
              name: text
              \"name:en\": text
              population: int8
            layers:
              places:
                minzoom: \"population > 1000000 ? 2 : 8\"
                attributes:
                  label: \"coalesce(feature['name:en'], name)\"
        "}), @r#"
        places: attributes.label from `feature['name:en']`
            "name:en" IS NOT NULL
        places: attributes.label from `name`
            "name:en" IS NULL AND "name" IS NOT NULL
        places: attributes.label null, every `coalesce` argument is null
            "name:en" IS NULL AND "name" IS NULL
        places: minzoom 2, `population > 1000000` is true
            "population" > 1000000
        places: minzoom 8, `population > 1000000` is false
            "population" <= 1000000
        places: minzoom, `population` is null in `population > 1000000`
            "population" IS NULL
        "#);
    }

    #[test]
    fn each_value_the_filter_allows_a_column_the_layer_reads_is_a_path() {
        insta::assert_snapshot!(listed(indoc! {"
            schema: public
            table: points
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            filter: \"type IN ('traffic_signals', 'stop', 'yield') AND lanes = 2\"
            properties:
              type: text
              lanes: int4
            layers:
              devices:
                attributes:
                  class: \"type == 'traffic_signals' ? 'TRAFFIC_LIGHT' : type.upperAscii()\"
        "}), @r#"
        devices: type 'traffic_signals' -> attributes.class 'TRAFFIC_LIGHT'
            "type" = 'traffic_signals'
        devices: type 'stop' -> attributes.class 'STOP'
            "type" = 'stop'
        devices: type 'yield' -> attributes.class 'YIELD'
            "type" = 'yield'
        "#);
    }

    #[test]
    fn a_filter_equal_to_one_value_gives_that_value_a_path() {
        insta::assert_snapshot!(listed(indoc! {"
            schema: public
            table: points
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            filter: \"type = 'stop'\"
            properties:
              type: text
            layers:
              devices:
                attributes:
                  class: \"type.upperAscii()\"
        "}), @r#"
        devices: type 'stop' -> attributes.class 'STOP'
            "type" = 'stop'
        "#);
    }

    #[test]
    fn a_filter_or_gives_no_value_a_path() {
        insta::assert_snapshot!(listed(indoc! {"
            schema: public
            table: points
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            filter: \"type = 'stop' OR type = 'yield'\"
            properties:
              type: text
            layers:
              devices:
                attributes:
                  class: \"type.upperAscii()\"
        "}), @"
        devices: every feature
            TRUE
        ");
    }

    #[test]
    fn a_filter_not_in_gives_no_value_a_path() {
        insta::assert_snapshot!(listed(indoc! {"
            schema: public
            table: points
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            filter: \"type NOT IN ('stop', 'yield')\"
            properties:
              type: text
            layers:
              devices:
                attributes:
                  class: \"type.upperAscii()\"
        "}), @"
        devices: every feature
            TRUE
        ");
    }

    #[test]
    fn whole_numbers_in_a_filter_are_integer_values() {
        insta::assert_snapshot!(listed(indoc! {"
            schema: public
            table: points
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            filter: \"lanes IN (1, 2)\"
            properties:
              lanes: int4
            layers:
              devices:
                attributes:
                  class: \"lanes * 2\"
        "}), @r#"
        devices: lanes 1 -> attributes.class 2
            "lanes" = 1
        devices: lanes 2 -> attributes.class 4
            "lanes" = 2
        "#);
    }

    #[test]
    fn fractions_in_a_filter_are_float_values() {
        insta::assert_snapshot!(listed(indoc! {"
            schema: public
            table: points
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            filter: \"width IN (2.5, 3.75)\"
            properties:
              width: float8
            layers:
              devices:
                attributes:
                  class: \"width * 2.0\"
        "}), @r#"
        devices: width 2.5 -> attributes.class 5.0
            ("width")::float8 = ('2.5')::float8
        devices: width 3.75 -> attributes.class 7.5
            ("width")::float8 = ('3.75')::float8
        "#);
    }

    #[test]
    fn a_column_the_filter_compares_otherwise_gives_no_value_a_path() {
        insta::assert_snapshot!(listed(indoc! {"
            schema: public
            table: points
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            filter: \"lanes > 2\"
            properties:
              lanes: int4
            layers:
              devices:
                attributes:
                  class: \"lanes * 2\"
        "}), @"
        devices: every feature
            TRUE
        ");
    }

    #[test]
    fn a_choice_sql_cannot_express_cannot_be_queried() {
        insta::assert_snapshot!(listed(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            properties:
              name: text
            layers:
              places:
                attributes:
                  short: \"name + '!' == 'A!' ? 'A' : name\"
        "}), @r#"
        places: attributes.short 'A', `(name + '!') == 'A!'` is true
            cannot query: uses `+`, which has no SQL equivalent
        places: attributes.short `name`, `(name + '!') == 'A!'` is false
            cannot query: uses `+`, which has no SQL equivalent
        places: attributes.short, `name` is null in `(name + '!') == 'A!'`
            "name" IS NULL
        "#);
    }

    #[test]
    fn the_choices_of_a_rule_attribute_take_only_the_rows_of_the_rule() {
        insta::assert_snapshot!(listed(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            properties:
              class: text
              name: text
              name_en: text
            layers:
              places:
                rules:
                  - where: \"class == 'city'\"
                    attributes:
                      label: \"coalesce(name_en, name)\"
        "}), @r#"
        places: rule 1 `where: class == 'city'`
            "class" = 'city'
        places: no rule matches
            "class" IS DISTINCT FROM 'city'
        places: rule 1 attributes.label from `name_en`
            "class" = 'city' AND "name_en" IS NOT NULL
        places: rule 1 attributes.label from `name`
            "class" = 'city' AND "name_en" IS NULL AND "name" IS NOT NULL
        places: rule 1 attributes.label null, every `coalesce` argument is null
            "class" = 'city' AND "name_en" IS NULL AND "name" IS NULL
        "#);
    }
}
