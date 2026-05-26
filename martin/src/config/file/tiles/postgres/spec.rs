//! A resolved tile-source description produced by `discover`, before it is instantiated into a running [`PostgresSource`].

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash as _, Hasher as _};

use martin_core::tiles::postgres::PostgresSqlInfo;

use crate::config::file::postgres::{FunctionInfo, TableInfo};

/// A resolved tile-source description: catalog metadata merged with config and the id already resolved, ready to be instantiated into a running source.
#[derive(Clone, Debug)]
pub enum SourceSpec {
    /// A table source. Its SQL query and bounds are deferred to instantiate.
    Table(TableInfo),
    /// A function source. Its SQL is already produced by the catalog query.
    Function(FunctionInfo, PostgresSqlInfo),
}

impl SourceSpec {
    /// A `u64` content hash over the fields that affect served tile bytes or metadata, used as the change-detection version in a reload diff.
    ///
    /// Two specs that would serve identical tiles hash equal, so an idle re-discover registers as "no change".
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        match self {
            Self::Table(info) => {
                0u8.hash(&mut hasher);
                info.layer_id.hash(&mut hasher);
                info.schema.hash(&mut hasher);
                info.table.hash(&mut hasher);
                info.srid.hash(&mut hasher);
                info.geometry_column.hash(&mut hasher);
                info.id_column.hash(&mut hasher);
                info.minzoom.hash(&mut hasher);
                info.maxzoom.hash(&mut hasher);
                info.extent.hash(&mut hasher);
                info.buffer.hash(&mut hasher);
                info.clip_geom.hash(&mut hasher);
                info.geometry_type.hash(&mut hasher);
                info.properties.hash(&mut hasher);
                hash_tilejson(info.tilejson.as_ref(), &mut hasher);
            }
            Self::Function(info, sql) => {
                1u8.hash(&mut hasher);
                info.schema.hash(&mut hasher);
                info.function.hash(&mut hasher);
                info.minzoom.hash(&mut hasher);
                info.maxzoom.hash(&mut hasher);
                hash_tilejson(info.tilejson.as_ref(), &mut hasher);
                sql.sql_query.hash(&mut hasher);
                sql.signature.hash(&mut hasher);
            }
        }
        hasher.finish()
    }
}

/// Hash the SQL-`COMMENT` `TileJSON` via its canonical string form, since `serde_json::Value` does not implement `Hash`.
/// `serde_json`'s default object representation is key-sorted, so the rendering is stable for equal values.
fn hash_tilejson(tilejson: Option<&serde_json::Value>, hasher: &mut DefaultHasher) {
    match tilejson {
        Some(value) => {
            1u8.hash(hasher);
            value.to_string().hash(hasher);
        }
        None => 0u8.hash(hasher),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::num::NonZeroU32;

    use rstest::rstest;
    use tilejson::Bounds;

    use super::*;
    use crate::config::file::CachePolicy;
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    use crate::config::primitives::AutoOption;

    /// Mutates one field of a [`TableInfo`] in place, so a test can isolate its effect on the fingerprint.
    type TableMutator = fn(&mut TableInfo);
    /// Mutates one field of a function spec (info or SQL) in place.
    type FunctionMutator = fn(&mut FunctionInfo, &mut PostgresSqlInfo);

    fn table(schema: &str, table: &str) -> TableInfo {
        TableInfo {
            schema: schema.to_string(),
            table: table.to_string(),
            geometry_column: "geom".to_string(),
            srid: 4326,
            ..Default::default()
        }
    }

    /// A table info with every fingerprinted (included) field set, so a test can flip exactly one field and observe the effect.
    fn full_table() -> TableInfo {
        TableInfo {
            layer_id: Some("layer".to_string()),
            schema: "public".to_string(),
            table: "roads".to_string(),
            srid: 4326,
            geometry_column: "geom".to_string(),
            id_column: Some("gid".to_string()),
            minzoom: Some(0),
            maxzoom: Some(14),
            extent: NonZeroU32::new(4096),
            buffer: Some(64),
            clip_geom: Some(true),
            geometry_type: Some("LINESTRING".to_string()),
            properties: Some(BTreeMap::from([("name".to_string(), "text".to_string())])),
            tilejson: Some(serde_json::json!({ "attribution": "abc" })),
            ..Default::default()
        }
    }

    fn fp(info: TableInfo) -> u64 {
        SourceSpec::Table(info).fingerprint()
    }

    #[test]
    fn equal_table_specs_hash_equal() {
        let a = SourceSpec::Table(table("public", "roads"));
        let b = SourceSpec::Table(table("public", "roads"));
        assert_eq!(a.fingerprint(), b.fingerprint());
    }

    #[rstest]
    #[case::layer_id(|t: &mut TableInfo|t.layer_id = Some("other".to_string()))]
    #[case::schema(|t: &mut TableInfo|t.schema = "other".to_string())]
    #[case::table(|t: &mut TableInfo|t.table = "other".to_string())]
    #[case::srid(|t: &mut TableInfo|t.srid = 3857)]
    #[case::geometry_column(|t: &mut TableInfo|t.geometry_column = "shape".to_string())]
    #[case::id_column(|t: &mut TableInfo|t.id_column = Some("fid".to_string()))]
    #[case::minzoom(|t: &mut TableInfo|t.minzoom = Some(2))]
    #[case::maxzoom(|t: &mut TableInfo|t.maxzoom = Some(18))]
    #[case::extent(|t: &mut TableInfo|t.extent = NonZeroU32::new(2048))]
    #[case::buffer(|t: &mut TableInfo|t.buffer = Some(128))]
    #[case::clip_geom(|t: &mut TableInfo|t.clip_geom = Some(false))]
    #[case::geometry_type(|t: &mut TableInfo|t.geometry_type = Some("POINT".to_string()))]
    #[case::properties(|t: &mut TableInfo|{
        t.properties = Some(BTreeMap::from([("kind".to_string(), "text".to_string())]));
    })]
    #[case::tilejson(|t: &mut TableInfo|{
        t.tilejson = Some(serde_json::json!({ "attribution": "xyz" }));
    })]
    fn flipping_an_included_field_changes_fingerprint(#[case] mutate: TableMutator) {
        let mut info = full_table();
        mutate(&mut info);
        assert_ne!(
            fp(info),
            fp(full_table()),
            "changing an included field should change the fingerprint"
        );
    }

    #[rstest]
    #[case::bounds(|t: &mut TableInfo|t.bounds = Some(Bounds::new(-1.0, -2.0, 3.0, 4.0)))]
    #[case::relkind(|t: &mut TableInfo|t.relkind = Some('m'))]
    #[case::geometry_index(|t: &mut TableInfo|t.geometry_index = Some(false))]
    #[case::prop_mapping(|t: &mut TableInfo|{
        t.prop_mapping
            .insert("name".to_string(), "name_col".to_string());
    })]
    #[case::cache(|t: &mut TableInfo|t.cache = Some(CachePolicy::disabled()))]
    #[case::unrecognized(|t: &mut TableInfo|{
        t.unrecognized.insert(
            "extra".to_string(),
            serde_yaml::Value::String("v".to_string()),
        );
    })]
    fn flipping_an_excluded_field_keeps_fingerprint(#[case] mutate: TableMutator) {
        let mut info = full_table();
        mutate(&mut info);
        assert_eq!(
            fp(info),
            fp(full_table()),
            "changing an excluded field must NOT change the fingerprint"
        );
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[rstest]
    #[case::convert_to_mlt(|t: &mut TableInfo|t.convert_to_mlt = Some(AutoOption::Disabled))]
    #[case::convert_to_mvt(|t: &mut TableInfo|t.convert_to_mvt = Some(AutoOption::Disabled))]
    fn flipping_an_excluded_conversion_field_keeps_fingerprint(#[case] mutate: TableMutator) {
        let mut info = full_table();
        mutate(&mut info);
        assert_eq!(
            fp(info),
            fp(full_table()),
            "changing an excluded conversion field must NOT change the fingerprint"
        );
    }

    fn full_function() -> (FunctionInfo, PostgresSqlInfo) {
        let info = FunctionInfo {
            schema: "public".to_string(),
            function: "tiles".to_string(),
            minzoom: Some(0),
            maxzoom: Some(14),
            tilejson: Some(serde_json::json!({ "attribution": "abc" })),
            ..Default::default()
        };
        let sql = PostgresSqlInfo::new(
            "SELECT mvt FROM public.tiles($1, $2, $3)".to_string(),
            false,
            "public.tiles(integer,integer,integer)".to_string(),
        );
        (info, sql)
    }

    fn ffp(info: FunctionInfo, sql: PostgresSqlInfo) -> u64 {
        SourceSpec::Function(info, sql).fingerprint()
    }

    #[test]
    fn equal_function_specs_hash_equal() {
        let (info, sql) = full_function();
        let (info2, sql2) = full_function();
        assert_eq!(ffp(info, sql), ffp(info2, sql2));
    }

    #[rstest]
    #[case::schema(|f: &mut FunctionInfo, _: &mut PostgresSqlInfo|f.schema = "other".to_string())]
    #[case::function(|f: &mut FunctionInfo, _: &mut PostgresSqlInfo|f.function = "other".to_string())]
    #[case::minzoom(|f: &mut FunctionInfo, _: &mut PostgresSqlInfo|f.minzoom = Some(3))]
    #[case::maxzoom(|f: &mut FunctionInfo, _: &mut PostgresSqlInfo|f.maxzoom = Some(20))]
    #[case::tilejson(|f: &mut FunctionInfo, _: &mut PostgresSqlInfo|{
        f.tilejson = Some(serde_json::json!({ "attribution": "xyz" }));
    })]
    #[case::sql_query(|_: &mut FunctionInfo, s: &mut PostgresSqlInfo|s.sql_query = "SELECT 1".to_string())]
    #[case::signature(|_: &mut FunctionInfo, s: &mut PostgresSqlInfo|s.signature = "public.tiles(text)".to_string())]
    fn flipping_an_included_function_field_changes_fingerprint(#[case] mutate: FunctionMutator) {
        let (base_info, base_sql) = full_function();
        let base = ffp(base_info, base_sql);
        let (mut info, mut sql) = full_function();
        mutate(&mut info, &mut sql);
        assert_ne!(
            ffp(info, sql),
            base,
            "changing an included field should change the function fingerprint"
        );
    }

    #[rstest]
    #[case::bounds(|f: &mut FunctionInfo, _: &mut PostgresSqlInfo|f.bounds = Some(Bounds::new(-1.0, -2.0, 3.0, 4.0)))]
    #[case::cache(|f: &mut FunctionInfo, _: &mut PostgresSqlInfo|f.cache = Some(CachePolicy::disabled()))]
    #[case::unrecognized(|f: &mut FunctionInfo, _: &mut PostgresSqlInfo|{
        f.unrecognized.insert(
            "extra".to_string(),
            serde_yaml::Value::String("v".to_string()),
        );
    })]
    fn flipping_an_excluded_function_field_keeps_fingerprint(#[case] mutate: FunctionMutator) {
        let (base_info, base_sql) = full_function();
        let base = ffp(base_info, base_sql);
        let (mut info, mut sql) = full_function();
        mutate(&mut info, &mut sql);
        assert_eq!(
            ffp(info, sql),
            base,
            "changing an excluded field must NOT change the function fingerprint"
        );
    }

    #[test]
    fn table_and_function_with_same_names_hash_differently() {
        let table = SourceSpec::Table(table("public", "tiles"));
        let (info, sql) = full_function();
        let function = SourceSpec::Function(
            FunctionInfo {
                function: "tiles".to_string(),
                ..info
            },
            sql,
        );
        assert_ne!(table.fingerprint(), function.fingerprint());
    }
}
