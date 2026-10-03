//! Tile features read one row per feature

use compact_str::CompactString;
use deadpool_postgres::tokio_postgres::Row;
use mlt_core::PropValue;
use mlt_core::geo_types::Geometry;
use serde_json::Value;

use crate::tiles::postgres::PostgresError::{PostgresError as PgError, UnsupportedPropertyType};
use crate::tiles::postgres::{PostgresResult, parse_tile_wkb};

/// One feature of a tile, in tile coordinate space.
#[derive(Debug, Clone, PartialEq)]
pub struct PostgresFeature {
    /// The feature id, when the source has an id column that held a non-negative value.
    pub id: Option<u64>,
    /// The geometry, already clipped and projected into tile space.
    pub geometry: Geometry<i32>,
    /// One M ordinate per vertex of [`Self::geometry`], when the source geometry is measured.
    pub m_values: Option<Vec<f64>>,
    /// The property columns, in the order the query selected them.
    pub properties: Vec<(CompactString, PostgresProperty)>,
}

/// One property column's value for one feature.
#[derive(Debug, Clone, PartialEq)]
pub enum PostgresProperty {
    /// A value of a type a tile column holds.
    Value(PropValue),
    /// A `jsonb` document, whose shape the tile format decides how to keep.
    Json(Option<Value>),
}

impl From<PropValue> for PostgresProperty {
    fn from(value: PropValue) -> Self {
        Self::Value(value)
    }
}

/// Everything one tile's worth of rows makes up: a single layer of features.
#[derive(Debug, Clone, PartialEq)]
pub struct PostgresTileFeatures {
    /// The name of the layer the features belong to.
    pub layer_name: String,
    /// The tile extent the geometries are expressed in.
    pub extent: u32,
    /// The features, in the order the rows arrived.
    pub features: Vec<PostgresFeature>,
}

/// A `PostgreSQL` column type that reaches a tile as a typed property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PropType {
    Bool,
    Int16,
    Int32,
    Int64,
    Float,
    Double,
    Text,
    Json,
}

impl PropType {
    /// The type a column of this `PostgreSQL` type holds, or `None` for one that needs a cast.
    fn of(pg_type: &str) -> Option<Self> {
        Some(match pg_type {
            "bool" => Self::Bool,
            "int2" => Self::Int16,
            "int4" => Self::Int32,
            "int8" => Self::Int64,
            "float4" => Self::Float,
            "float8" => Self::Double,
            "text" | "varchar" | "bpchar" | "name" => Self::Text,
            "jsonb" => Self::Json,
            _ => return None,
        })
    }
}

/// Whether a column of this `PostgreSQL` type reaches a tile as a typed property.
#[must_use]
pub fn is_typed_property(pg_type: &str) -> bool {
    PropType::of(pg_type).is_some()
}

/// Decodes the rows of one tile query into features.
pub(crate) fn features_from_rows(
    rows: &[Row],
    has_id_column: bool,
) -> PostgresResult<Vec<PostgresFeature>> {
    let first_property = 1 + usize::from(has_id_column);
    let mut features = Vec::with_capacity(rows.len());
    for row in rows {
        let wkb: Option<&[u8]> = row
            .try_get(0)
            .map_err(|e| PgError(e, "reading a tile feature's geometry"))?;
        let Some(wkb) = wkb else {
            continue;
        };
        let (geometry, m_values) = parse_tile_wkb(wkb)?;
        let id = if has_id_column {
            feature_id(row)?
        } else {
            None
        };
        let mut properties = Vec::with_capacity(row.columns().len().saturating_sub(first_property));
        for idx in first_property..row.columns().len() {
            let column = &row.columns()[idx];
            properties.push((
                CompactString::new(column.name()),
                property(row, idx, "reading a tile property")?,
            ));
        }
        features.push(PostgresFeature {
            id,
            geometry,
            m_values,
            properties,
        });
    }
    Ok(features)
}

/// The feature id in the row's second column, following `ST_AsMVT`.
fn feature_id(row: &Row) -> PostgresResult<Option<u64>> {
    let column = &row.columns()[1];
    let value = property(row, 1, "reading a tile feature's id")?;
    let PostgresProperty::Value(PropValue::I64(value)) = value else {
        return Err(UnsupportedPropertyType {
            column: column.name().to_owned(),
            pg_type: column.type_().name().to_owned(),
        });
    };
    Ok(value.and_then(|v| u64::try_from(v).ok()))
}

/// One column's value, typed by what the column's runtime type says it holds.
fn property(row: &Row, idx: usize, context: &'static str) -> PostgresResult<PostgresProperty> {
    let column = &row.columns()[idx];
    let read = |e| PgError(e, context);
    let Some(prop_type) = PropType::of(column.type_().name()) else {
        return Err(UnsupportedPropertyType {
            column: column.name().to_owned(),
            pg_type: column.type_().name().to_owned(),
        });
    };
    Ok(PostgresProperty::Value(match prop_type {
        PropType::Json => return Ok(PostgresProperty::Json(row.try_get(idx).map_err(read)?)),
        PropType::Bool => PropValue::Bool(row.try_get(idx).map_err(read)?),
        PropType::Int16 => PropValue::I64(
            row.try_get::<_, Option<i16>>(idx)
                .map_err(read)?
                .map(i64::from),
        ),
        PropType::Int32 => PropValue::I64(
            row.try_get::<_, Option<i32>>(idx)
                .map_err(read)?
                .map(i64::from),
        ),
        PropType::Int64 => PropValue::I64(row.try_get(idx).map_err(read)?),
        PropType::Float => PropValue::F32(row.try_get(idx).map_err(read)?),
        PropType::Double => PropValue::F64(row.try_get(idx).map_err(read)?),
        PropType::Text => PropValue::Str(row.try_get(idx).map_err(read)?),
    }))
}
