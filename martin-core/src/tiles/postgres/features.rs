//! Tile features read one row per feature, instead of as a single MVT blob.
//!
//! `ST_AsMVT` serializes a whole tile inside `PostgreSQL`, which a consumer that wants any other
//! tile format has to take apart again. The types here are the other shape of the same data: the
//! rows of a [`PostgresRowQuery`](crate::tiles::postgres::PostgresRowQuery), decoded but not yet
//! serialized into any tile format.

use compact_str::CompactString;
use deadpool_postgres::tokio_postgres::Row;

use crate::tiles::postgres::PostgresError::{PostgresError as PgError, UnsupportedPropertyType};
use crate::tiles::postgres::{PostgresResult, TileGeometry, parse_tile_wkb};

/// One property value of one feature, or the `NULL` the column held for it.
///
/// The variants are the `PostgreSQL` types `ST_AsMVT` encodes as MVT properties; every other
/// column type is rejected rather than stringified.
#[derive(Debug, Clone, PartialEq)]
pub enum PostgresPropValue {
    /// A `bool` column.
    Bool(Option<bool>),
    /// An `int2`, `int4` or `int8` column.
    Int(Option<i64>),
    /// A `float4` column.
    Float(Option<f32>),
    /// A `float8` column.
    Double(Option<f64>),
    /// A `text`, `varchar`, `bpchar` or `name` column.
    Text(Option<String>),
}

/// One feature of a tile, in tile coordinate space.
#[derive(Debug, Clone, PartialEq)]
pub struct PostgresFeature {
    /// The feature id, when the source has an id column that held a non-negative value.
    pub id: Option<u64>,
    /// The geometry, already clipped and projected into tile space by `ST_AsMVTGeom`.
    pub geometry: TileGeometry,
    /// The property columns, in the order the query selected them.
    pub properties: Vec<(CompactString, PostgresPropValue)>,
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

/// Whether a column of this `PostgreSQL` type reaches a tile as a typed property.
///
/// `ST_AsMVT` writes every other type through the type's text output function, so a query that
/// wants the same properties has to cast those columns to `text` itself.
#[must_use]
pub fn is_typed_property(pg_type: &str) -> bool {
    matches!(
        pg_type,
        "bool"
            | "int2"
            | "int4"
            | "int8"
            | "float4"
            | "float8"
            | "text"
            | "varchar"
            | "bpchar"
            | "name"
    )
}

/// Decodes the rows of one tile query into features.
///
/// The first column is the geometry, followed by the id column when `has_id_column`, followed by
/// the property columns. Rows whose geometry is `NULL` are dropped, which is what `ST_AsMVT` does
/// with a feature `ST_AsMVTGeom` placed outside the tile.
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
        let geometry = parse_tile_wkb(wkb)?;
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
                property_value(row, idx, column.name(), column.type_().name())?,
            ));
        }
        features.push(PostgresFeature {
            id,
            geometry,
            properties,
        });
    }
    Ok(features)
}

/// The feature id in the row's second column, following `ST_AsMVT`.
///
/// `PostGIS` only accepts an integer id column and only emits ids it can express as a `uint64`,
/// so a `NULL` or negative value leaves the feature without one.
fn feature_id(row: &Row) -> PostgresResult<Option<u64>> {
    let column = &row.columns()[1];
    let value = match column.type_().name() {
        "int2" => row
            .try_get::<_, Option<i16>>(1)
            .map(|v| v.map(i64::from))
            .map_err(|e| PgError(e, "reading a tile feature's id")),
        "int4" => row
            .try_get::<_, Option<i32>>(1)
            .map(|v| v.map(i64::from))
            .map_err(|e| PgError(e, "reading a tile feature's id")),
        "int8" => row
            .try_get::<_, Option<i64>>(1)
            .map_err(|e| PgError(e, "reading a tile feature's id")),
        other => Err(UnsupportedPropertyType {
            column: column.name().to_owned(),
            pg_type: other.to_owned(),
        }),
    }?;
    Ok(value.and_then(|v| u64::try_from(v).ok()))
}

/// One property value, typed by what the column's runtime type says it holds.
fn property_value(
    row: &Row,
    idx: usize,
    name: &str,
    pg_type: &str,
) -> PostgresResult<PostgresPropValue> {
    let read = |e| PgError(e, "reading a tile property");
    Ok(match pg_type {
        "bool" => PostgresPropValue::Bool(row.try_get(idx).map_err(read)?),
        "int2" => PostgresPropValue::Int(
            row.try_get::<_, Option<i16>>(idx)
                .map_err(read)?
                .map(i64::from),
        ),
        "int4" => PostgresPropValue::Int(
            row.try_get::<_, Option<i32>>(idx)
                .map_err(read)?
                .map(i64::from),
        ),
        "int8" => PostgresPropValue::Int(row.try_get(idx).map_err(read)?),
        "float4" => PostgresPropValue::Float(row.try_get(idx).map_err(read)?),
        "float8" => PostgresPropValue::Double(row.try_get(idx).map_err(read)?),
        "text" | "varchar" | "bpchar" | "name" => {
            PostgresPropValue::Text(row.try_get(idx).map_err(read)?)
        }
        other => {
            return Err(UnsupportedPropertyType {
                column: name.to_owned(),
                pg_type: other.to_owned(),
            });
        }
    })
}
