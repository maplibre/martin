//! A backend of tables held in memory, which runs `martin test` without a database.

use std::collections::BTreeMap;

use martin_tilegen::props::{ByteString, KeyId, Prop, SourceStr};
use martin_tilegen::source::{Crs, FeatureBatch, Geometry, MemorySource, SourceFeature};
use mlt_core::geo_types;
use serde_json::Value;
use wkt::TryFromWkt as _;

use crate::generate::spec::backend::{RowKey, Rows, TestBackend, TestTable};
use crate::generate::{GenerateError, GenerateResult};

/// A table and its rows, each a column-to-value map with its geometry as lon/lat WKT under `geometry`, without which
/// the row is left out.
#[derive(Clone, Debug)]
pub struct MemoryTable {
    pub table: TestTable,
    pub rows: Vec<BTreeMap<String, Value>>,
}

/// Tables held in memory, whose layers are lowered already.
#[derive(Clone, Debug, Default)]
pub struct Memory {
    pub tables: Vec<MemoryTable>,
}

impl TestBackend for Memory {
    type Scan = MemorySource;

    /// The tables as given, whatever `max_zoom`.
    fn tables(&self, _max_zoom: u8) -> GenerateResult<Vec<TestTable>> {
        Ok(self.tables.iter().map(|t| t.table.clone()).collect())
    }

    /// Every row, as nothing pushes layer filters down.
    #[expect(
        clippy::unused_async_trait_impl,
        reason = "the rows are at hand, other backends wait for theirs"
    )]
    async fn scan(&self, table: &TestTable, key: Option<RowKey>, _rows: Rows) -> GenerateResult<MemorySource> {
        let Some(memory) = self.tables.iter().find(|t| t.table.source == table.source) else {
            let known = self.tables.iter().map(|t| t.table.source.as_str()).collect::<Vec<_>>();
            return Err(GenerateError::UnknownSource(table.source.clone(), known.join(", ")));
        };
        let plan = table.plan()?;
        let columns = plan.columns(0);
        let row_key = |row: &BTreeMap<String, Value>| row.get(table.key.as_deref()?)?.as_u64();
        let features = memory
            .rows
            .iter()
            .filter(|row| key.is_none() || row_key(row) == key)
            .filter_map(|row| {
                Some(SourceFeature {
                    id: row_key(row),
                    geometry: geometry(row.get("geometry")?.as_str()?)?,
                    props: (0..)
                        .zip(columns)
                        .filter_map(|(i, column)| Some((KeyId::new(i), prop(row.get(column)?)?)))
                        .collect(),
                })
            })
            .collect();
        Ok(MemorySource {
            batches: vec![FeatureBatch {
                table: 0,
                partition: 0,
                first_row: 0,
                crs: Crs::Wgs84,
                features,
            }],
        })
    }
}

/// A JSON value as a property, `None` for `null`, arrays and objects.
fn prop(value: &Value) -> Option<Prop<SourceStr>> {
    match value {
        Value::Bool(v) => Some(Prop::Bool(*v)),
        Value::Number(v) => v.as_i64().map(Prop::I64).or_else(|| v.as_f64().map(Prop::F64)),
        Value::String(v) => Some(Prop::Str(ByteString::from(v.as_str()))),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

/// A WKT geometry, `None` when it cannot be parsed or has no feature geometry type.
fn geometry(text: &str) -> Option<Geometry> {
    #[expect(clippy::wildcard_enum_match_arm, reason = "no other geometry is a feature geometry")]
    Some(match geo_types::Geometry::<f64>::try_from_wkt_str(text).ok()? {
        geo_types::Geometry::Point(point) => Geometry::Point(point.0),
        geo_types::Geometry::LineString(line) => Geometry::Line(line),
        geo_types::Geometry::Polygon(polygon) => Geometry::Polygon(polygon),
        geo_types::Geometry::MultiPoint(points) => Geometry::Points(points.0.into_iter().map(|p| p.0).collect()),
        geo_types::Geometry::MultiLineString(lines) => Geometry::Lines(lines.0),
        geo_types::Geometry::MultiPolygon(polygons) => Geometry::Polygons(polygons.0),
        _ => return None,
    })
}
