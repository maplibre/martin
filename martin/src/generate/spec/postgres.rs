//! The `PostgreSQL` backend of `martin test`: the table sources of one database, and the SQL selecting the rows on
//! each path through their layers.

use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::path::Path;
use std::time::Duration;

use martin_core::tiles::postgres::PostgresPool;
use martin_tile_utils::EARTH_CIRCUMFERENCE;
use martin_tilegen::plan::{ColumnValues, Condition, GeometryType, Literal, Shape, SizeBand};
use postgres_protocol::escape::escape_identifier;

use crate::config::file::postgres::resolver::{condition_sql, scan_sql};
use crate::config::file::postgres::{PostgresAutoDiscoveryBuilder, SourceSpec, TableInfo};
use crate::config::file::{Config, TileGrids, read_config};
use crate::config::primitives::IdResolver;
use crate::config::primitives::env::OsEnv;
use crate::generate::layers::{LowerOptions, lower_table};
use crate::generate::postgres::{PgScanSource, ScanTable};
use crate::generate::spec::backend::{Find, RowKey, Rows, TestBackend, TestTable};
use crate::generate::{GenerateError, GenerateResult};
use crate::srv::RESERVED_KEYWORDS;

/// How long the search for one path's row may take.
const PATH_TIMEOUT: Duration = Duration::from_secs(120);

/// The table sources with layers of a configuration, and the database they come from.
pub struct Postgres {
    builder: PostgresAutoDiscoveryBuilder,
    tables: BTreeMap<String, TableInfo>,
}

impl Postgres {
    /// Reads the configuration and discovers its table sources.
    ///
    /// # Errors
    ///
    /// If the configuration cannot be read, or the database not reached.
    pub async fn load(config_path: &Path) -> GenerateResult<Self> {
        let mut config: Config = read_config(config_path, &OsEnv)?;
        config.finalize().await?;
        let grids = TileGrids::resolve(&config.tile_grids)?;
        let pg = match config.postgres.as_slice() {
            [] => return Err(GenerateError::NoSources),
            [pg] => pg,
            _ => return Err(GenerateError::MultipleDatabases),
        };
        let builder =
            PostgresAutoDiscoveryBuilder::new(pg, IdResolver::new(RESERVED_KEYWORDS), config.cache.policy(), &grids)
                .await?;
        let (specs, _) = builder.discover().await?;
        let tables = specs
            .into_iter()
            .filter_map(|(id, spec)| match spec {
                SourceSpec::Table(info) if info.layers.is_some() => Some((id, info)),
                SourceSpec::Table(_) | SourceSpec::Function(..) => None,
            })
            .collect();
        Ok(Self { builder, tables })
    }

    /// The pool of the sources' database.
    fn pool(&self) -> &PostgresPool {
        self.builder.pool()
    }

    /// The table of source `source`.
    fn info(&self, source: &str) -> GenerateResult<&TableInfo> {
        self.tables.get(source).ok_or_else(|| {
            let known = self.tables.keys().cloned().collect::<Vec<_>>().join(", ");
            GenerateError::UnknownSource(source.to_owned(), known)
        })
    }

    /// The table of source `source`, its `filter` narrowed to row `key`.
    fn row(&self, source: &str, key: RowKey) -> GenerateResult<TableInfo> {
        let mut info = self.info(source)?.clone();
        let Some(column) = &info.id_column else {
            return Err(GenerateError::NoIdColumn(source.to_owned()));
        };
        let row = format!("\"{}\" = {key}", info.table_column(column).replace('"', "\"\""));
        info.filter = Some(match info.filter.take() {
            Some(filter) => format!("({filter}) AND ({row})"),
            None => row,
        });
        Ok(info)
    }

    /// The key of a row of `info` on `rows` with geometry of `shape`, reading the rows as the scan does.
    async fn search(&self, info: &TableInfo, rows: &Condition, shape: Option<Shape>) -> Result<Option<RowKey>, String> {
        let id_column = info.id_column.as_ref().ok_or("the source has no `id_column`")?;
        let scan = scan_sql(info, &[], Some(rows), None).map_err(|e| e.to_string())?;
        let id = escape_identifier(id_column);
        let size = fmt::from_fn(|f| match shape {
            Some(shape) => write!(f, " AND {}", ShapeSql(info, shape)),
            None => Ok(()),
        });
        let query = format!(
            "SELECT s.{id}::int8 FROM ({}{size}) AS s WHERE s.{id}::int8 >= 0 LIMIT 1",
            scan.sql
        );
        let conn = self.pool().get().await.map_err(|e| e.to_string())?;
        let timeout = format!("SET statement_timeout = '{}s'", PATH_TIMEOUT.as_secs());
        conn.batch_execute(&timeout).await.map_err(|e| db_error(&e))?;
        let found = conn.query_opt(query.as_str(), &[]).await;
        conn.batch_execute("RESET statement_timeout")
            .await
            .map_err(|e| db_error(&e))?;
        let Some(row) = found.map_err(|e| db_error(&e))? else {
            return Ok(None);
        };
        let key = row.try_get::<_, i64>(0).map_err(|e| db_error(&e))?;
        RowKey::try_from(key).map(Some).map_err(|e| e.to_string())
    }
}

/// What `PostgreSQL` says went wrong, the client's error when it says nothing.
fn db_error(err: &(dyn std::error::Error + 'static)) -> String {
    err.source().map_or_else(|| err.to_string(), ToString::to_string)
}

impl TestBackend for Postgres {
    type Scan = PgScanSource;

    fn tables(&self, max_zoom: u8) -> GenerateResult<Vec<TestTable>> {
        let mut out = Vec::with_capacity(self.tables.len());
        for (id, info) in &self.tables {
            out.extend(test_table(id, info, max_zoom)?);
        }
        Ok(out)
    }

    /// Partitioned as bulk mode partitions a table, the scan of one row in a single partition.
    async fn scan(&self, table: &TestTable, key: Option<RowKey>, rows: Rows) -> GenerateResult<PgScanSource> {
        let (info, blocks) = match key {
            Some(key) => (self.row(&table.source, key)?, u32::MAX),
            None => (self.info(&table.source)?.clone(), PgScanSource::PARTITION_BLOCKS),
        };
        let scan = vec![ScanTable {
            info,
            def: table.def.clone(),
        }];
        let pool = self.pool().clone();
        Ok(match rows {
            Rows::Bulk => PgScanSource::new(pool, scan, blocks).await?,
            Rows::All => PgScanSource::every_row(pool, scan, blocks).await?,
        })
    }

    async fn find(&self, table: &TestTable, rows: &Condition, shape: Option<Shape>) -> Find {
        let Ok(info) = self.info(&table.source) else {
            return Find::Unsupported;
        };
        let selected = RowsSql(info, rows, shape).to_string();
        match self.search(info, rows, shape).await {
            Ok(key) => Find::Searched { rows: selected, key },
            Err(error) => Find::Failed { rows: selected, error },
        }
    }
}

/// Source `id` with table `info` as `martin test` reads it, at zooms 0 to `max_zoom`, or `None` when it has none of
/// them. The values the source's `filter` allows a column are known values of the column.
///
/// # Errors
///
/// If the source cannot be lowered.
pub fn test_table(id: &str, info: &TableInfo, max_zoom: u8) -> GenerateResult<Option<TestTable>> {
    let options = LowerOptions {
        zooms: 0..=max_zoom,
        bounds: None,
    };
    let Some(scan) = lower_table(id, info.clone(), &options)? else {
        return Ok(None);
    };
    Ok(Some(TestTable {
        source: id.to_owned(),
        def: scan.def,
        known: filter_values(info),
        key: info.id_column.clone(),
        geometry: table_geometry(info),
    }))
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
        test_table("source", &info, 14)
            .expect("lowers")
            .expect("has zooms")
            .paths()
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
