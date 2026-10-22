//! The `PostgreSQL` side of `martin test`: the paths through the layers of a table source, with the SQL selecting
//! the rows on each.

use std::fmt::{self, Display, Formatter};

use martin_tile_utils::EARTH_CIRCUMFERENCE;
use martin_tilegen::TileGenError;
use martin_tilegen::plan::{Branch, Condition, GeometryType, Plan, SizeBand};
use postgres_protocol::escape::escape_identifier;

use crate::config::file::postgres::TableInfo;
use crate::config::file::postgres::resolver::condition_sql;
use crate::generate::GenerateResult;
use crate::generate::layers::{LowerOptions, lower_table};
use crate::generate::postgres::ScanTable;

/// Every path through the layers of `info` at zooms 0 to `max_zoom`, without size bands for a source of points.
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
        .branches(0)?
        .into_iter()
        .filter(|branch| !(points && branch.size.is_some()))
        .collect())
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
pub struct RowsSql<'a>(pub &'a TableInfo, pub &'a Condition, pub Option<SizeBand>);

impl Display for RowsSql<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let Self(info, condition, size) = *self;
        let condition = condition_sql(info, condition);
        match size {
            Some(size) => write!(f, "({condition}) AND {}", SizeSql(info, size)),
            None => f.write_str(&condition),
        }
    }
}

/// The SQL condition on the geometries of a size band.
pub struct SizeSql<'a>(pub &'a TableInfo, pub SizeBand);

impl Display for SizeSql<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let Self(info, SizeBand { zoom, min, max }) = *self;
        let meters = |pixels: f64| pixels * EARTH_CIRCUMFERENCE / 256.0 / f64::from(1u32 << zoom);
        let geom = escape_identifier(&info.geometry_column);
        write!(
            f,
            "numrange({}, {}) @> (SELECT greatest(ST_XMax(b) - ST_XMin(b), ST_YMax(b) - ST_YMin(b))::numeric FROM (SELECT ST_Transform({geom}::geometry, 3857)::box2d AS b) AS bbox)",
            meters(min),
            meters(max)
        )
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
                    Ok(condition) => RowsSql(&info, condition, branch.size).to_string(),
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
        roads: rule 1 `class == 'motorway'`
            "class" = 'motorway'
        roads: rule 2 `class == 'path'`
            "class" IS DISTINCT FROM 'motorway' AND "class" = 'path'
        roads: no rule
            "class" IS DISTINCT FROM 'motorway' AND "class" IS DISTINCT FROM 'path'
        roads: kept only at z14 by `min_size_at_maxzoom`
            (ST_GeometryType("geom"::geometry) IN ('ST_LineString', 'ST_MultiLineString') OR ST_GeometryType("geom"::geometry) IN ('ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle')) AND numrange(0.5971642834779397, 19.10925707129407) @> (SELECT greatest(ST_XMax(b) - ST_XMin(b), ST_YMax(b) - ST_YMin(b))::numeric FROM (SELECT ST_Transform("geom"::geometry, 3857)::box2d AS b) AS bbox)
        roads: smaller than `min_size` at every zoom
            (ST_GeometryType("geom"::geometry) IN ('ST_LineString', 'ST_MultiLineString') OR ST_GeometryType("geom"::geometry) IN ('ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle')) AND numrange(0, 0.5971642834779397) @> (SELECT greatest(ST_XMax(b) - ST_XMin(b), ST_YMax(b) - ST_YMin(b))::numeric FROM (SELECT ST_Transform("geom"::geometry, 3857)::box2d AS b) AS bbox)
        "#);
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
        places: where `population > 1000`
            ST_GeometryType("geom"::geometry) IN ('ST_Point', 'ST_MultiPoint') AND "population" > 1000
        places: not where `population > 1000`
            ST_GeometryType("geom"::geometry) IN ('ST_Point', 'ST_MultiPoint') AND "population" <= 1000
        places: where `population > 1000` with `population` null
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
        places: every feature
            TRUE
        places: `coalesce(feature['name:en'], name)`: `feature['name:en']` is the first not null
            "name:en" IS NOT NULL
        places: `coalesce(feature['name:en'], name)`: `name` is the first not null
            "name:en" IS NULL AND "name" IS NOT NULL
        places: `coalesce(feature['name:en'], name)`: all null
            "name:en" IS NULL AND "name" IS NULL
        places: `population > 1000000 ? 2 : 8`: `population > 1000000` true
            "population" > 1000000
        places: `population > 1000000 ? 2 : 8`: `population > 1000000` false
            "population" <= 1000000
        places: `population > 1000000 ? 2 : 8`: `population > 1000000` fails with `population` null
            "population" IS NULL
        "#);
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
        places: every feature
            TRUE
        places: `name + '!' == 'A!' ? 'A' : name`: `(name + '!') == 'A!'` true
            cannot query: uses `+`, which has no SQL equivalent
        places: `name + '!' == 'A!' ? 'A' : name`: `(name + '!') == 'A!'` false
            cannot query: uses `+`, which has no SQL equivalent
        places: `name + '!' == 'A!' ? 'A' : name`: `(name + '!') == 'A!'` fails with `name` null
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
        places: rule 1 `class == 'city'`
            "class" = 'city'
        places: no rule
            "class" IS DISTINCT FROM 'city'
        places: `coalesce(name_en, name)`: `name_en` is the first not null
            "class" = 'city' AND "name_en" IS NOT NULL
        places: `coalesce(name_en, name)`: `name` is the first not null
            "class" = 'city' AND "name_en" IS NULL AND "name" IS NOT NULL
        places: `coalesce(name_en, name)`: all null
            "class" = 'city' AND "name_en" IS NULL AND "name" IS NULL
        "#);
    }
}
