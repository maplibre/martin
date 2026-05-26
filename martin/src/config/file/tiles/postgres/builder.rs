use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::num::NonZeroU32;

use itertools::Itertools as _;
use martin_core::tiles::BoxedSource;
use martin_core::tiles::postgres::{PostgresPool, PostgresResult, PostgresSource, PostgresSqlInfo};
use tracing::{debug, error, info, warn};

use crate::config::args::BoundsCalcType;
use crate::config::file::postgres::resolver::{
    query_available_function, query_available_tables, table_to_query,
};
use crate::config::file::postgres::utils::{find_info, find_kv_ignore_case, normalize_key};
use crate::config::file::postgres::{
    FuncInfoSources, FunctionInfo, POOL_SIZE_DEFAULT, PostgresCfgPublish, PostgresCfgPublishFuncs,
    PostgresConfig, PostgresInfo, SourceSpec, TableInfo, TableInfoSources,
};
use crate::config::file::{CachePolicy, ConfigFileError, ConfigFileResult, TileSourceWarning};
use crate::config::primitives::IdResolver;
use crate::config::primitives::OptBoolObj::{Bool, NoValue, Object};
use crate::config::primitives::OptOneMany::NoVals;

/// Builder for [`PostgresSource`]' auto-discovery of functions and tables.
#[derive(Debug)]
pub struct PostgresAutoDiscoveryBuilder {
    pool: PostgresPool,
    /// If a spatial table has SRID 0, then this SRID will be used as a fallback
    default_srid: Option<i32>,
    /// Fallback cache zoom bounds for sources that don't set their own
    default_cache: CachePolicy,
    /// Specify how bounds should be computed for the spatial PG tables
    auto_bounds: BoundsCalcType,
    /// Limit the number of geo features per tile.
    ///
    /// If the source table has more features than set here, they will not be included in the tile and the result will look "cut off"/incomplete.
    /// This feature allows to put a maximum latency bound on tiles with extreme amount of detail at the cost of not returning all data.
    /// It is sensible to set this limit if you have user generated/untrusted geodata, e.g. a lot of data points at [Null Island](https://en.wikipedia.org/wiki/Null_Island).
    ///
    /// Can be either a positive integer or unlimited if omitted.
    max_feature_count: Option<usize>,
    auto_functions: Option<PostgresAutoDiscoveryBuilderFunctions>,
    auto_tables: Option<PostgresAutoDiscoveryBuilderTables>,
    id_resolver: IdResolver,
    /// Associative arrays of table sources
    tables: TableInfoSources,
    functions: FuncInfoSources,
}

/// Configuration for auto-discovering `PostgreSQL` functions.
#[derive(Debug, PartialEq)]
#[cfg_attr(test, serde_with::skip_serializing_none, derive(serde::Serialize))]
pub struct PostgresAutoDiscoveryBuilderFunctions {
    schemas: Option<HashSet<String>>,
    source_id_format: String,
}

/// Configuration for auto-discovering `PostgreSQL` tables.
#[derive(Debug, Default, PartialEq)]
#[cfg_attr(test, serde_with::skip_serializing_none, derive(serde::Serialize))]
pub struct PostgresAutoDiscoveryBuilderTables {
    schemas: Option<HashSet<String>>,
    source_id_format: String,
    id_columns: Option<Vec<String>>,
    clip_geom: Option<bool>,
    buffer: Option<u32>,
    extent: Option<NonZeroU32>,
}

/// Combine `from_schema` field from the `config.auto_publish` and `config.auto_publish.tables/functions`
macro_rules! get_auto_schemas {
    ($config:expr, $typ:ident) => {
        if let Object(v) = &$config.auto_publish {
            match (&v.from_schemas, &v.$typ) {
                (NoVals, NoValue | Bool(_)) => None,
                (v, NoValue | Bool(_)) => v.opt_iter().map(|v| v.cloned().collect()),
                (NoVals, Object(v)) => v.from_schemas.opt_iter().map(|v| v.cloned().collect()),
                (v, Object(v2)) => {
                    let mut vals: HashSet<_> = v.iter().cloned().collect();
                    vals.extend(v2.from_schemas.iter().cloned());
                    Some(vals)
                }
            }
        } else {
            None
        }
    };
}

impl PostgresAutoDiscoveryBuilder {
    /// Creates a new `PostgreSQL` source builder from the [`PostgresConfig`].
    ///
    /// Duplicate names are deterministically converted to unique names.
    pub async fn new(
        config: &PostgresConfig,
        id_resolver: IdResolver,
        default_cache: CachePolicy,
    ) -> ConfigFileResult<Self> {
        let pool = PostgresPool::new(
            config
                .connection_string
                .as_ref()
                .expect("connection_string should be set after PostgresConfig::finalize()")
                .as_str(),
            config.ssl_certificates.ssl_cert.as_ref(),
            config.ssl_certificates.ssl_key.as_ref(),
            config.ssl_certificates.ssl_root_cert.as_ref(),
            config.pool_size.unwrap_or(POOL_SIZE_DEFAULT),
        )
        .await
        .map_err(ConfigFileError::PostgresPoolCreationFailed)?;

        let (auto_tables, auto_functions) = calc_auto(config);

        Ok(Self {
            pool,
            default_srid: config.default_srid,
            default_cache,
            auto_bounds: config.auto_bounds.unwrap_or_default(),
            max_feature_count: config.max_feature_count,
            id_resolver,
            tables: config.tables.clone().unwrap_or_default(),
            functions: config.functions.clone().unwrap_or_default(),
            auto_functions,
            auto_tables,
        })
    }

    /// Returns the bounds calculation type for this builder.
    #[must_use]
    pub fn auto_bounds(&self) -> BoundsCalcType {
        self.auto_bounds
    }

    /// ID under which this [`PostgresAutoDiscoveryBuilder`] is identified externally
    #[must_use]
    pub fn get_id(&self) -> &str {
        self.pool.get_id()
    }

    /// Resolves which tile sources should exist right now, returning a [`SourceSpec`] per id.
    ///
    /// The cheap half of discovery: one catalog round-trip per source kind, config merge, auto-publish filtering, and id resolution.
    /// Building each source's SQL and computing its bounds is [`instantiate`](Self::instantiate)'s job.
    pub async fn discover(
        &self,
    ) -> PostgresResult<(BTreeMap<String, SourceSpec>, Vec<TileSourceWarning>)> {
        let mut specs = BTreeMap::new();
        let mut warnings = Vec::new();
        self.discover_tables(&mut specs, &mut warnings).await?;
        self.discover_functions(&mut specs, &mut warnings).await?;
        Ok((specs, warnings))
    }

    /// Catalog query + config merge + auto-publish + id resolution for tables, inserting a [`SourceSpec::Table`] per id.
    async fn discover_tables(
        &self,
        specs: &mut BTreeMap<String, SourceSpec>,
        warnings: &mut Vec<TileSourceWarning>,
    ) -> PostgresResult<()> {
        let restrict_to_tables = if self.auto_tables.is_none() {
            Some(self.configured_tables())
        } else {
            None
        };
        let mut db_tables_info = query_available_tables(&self.pool, restrict_to_tables).await?;

        // Match configured table sources against the discovered catalog.
        let mut used = HashSet::<(&str, &str, &str)>::new();
        for (id, cfg_inf) in &self.tables {
            match self.build_one_table_info(&db_tables_info, id, cfg_inf) {
                Ok(merged_inf) => {
                    if !used.insert((&cfg_inf.schema, &cfg_inf.table, &cfg_inf.geometry_column)) {
                        warn!(
                            source.id = %id,
                            schema = %cfg_inf.schema,
                            table = %cfg_inf.table,
                            geometry_column = %cfg_inf.geometry_column,
                            "Configured duplicate source: multiple config entries resolve to the same table and geometry column"
                        );
                    }
                    let id2 = self.resolve_id(id, &merged_inf);
                    warn_on_rename(id, &id2, "Table");
                    specs.insert(id2, SourceSpec::Table(merged_inf));
                }
                Err(error) => warnings.push(TileSourceWarning::SourceError {
                    source_id: id.clone(),
                    error,
                }),
            }
        }

        // Auto-publish remaining tables, sorted for deterministic id resolution.
        if let Some(auto_tables) = &self.auto_tables {
            let schemas = auto_tables
                .schemas
                .clone()
                .unwrap_or_else(|| db_tables_info.keys().cloned().collect());
            info!(
                schemas = %schemas.iter().sorted().join(", "),
                source_id_format = %auto_tables.source_id_format,
                "Auto-publishing tables"
            );
            for schema in schemas.iter().sorted() {
                let Some(schema) = normalize_key(&db_tables_info, schema, "schema", "") else {
                    continue;
                };
                let db_tables = db_tables_info.remove(&schema).expect(
                    "schema should be present in db_tables_info after normalize_key lookup",
                );
                for (table, geoms) in db_tables.into_iter().sorted_by(by_key) {
                    for (geom_column, mut db_inf) in geoms.into_iter().sorted_by(by_key) {
                        if used.contains(&(schema.as_str(), table.as_str(), geom_column.as_str())) {
                            continue;
                        }
                        let source_id = auto_tables
                            .source_id_format
                            .replace("{schema}", &schema)
                            .replace("{table}", &table)
                            .replace("{column}", &geom_column);
                        let id2 = self.resolve_id(&source_id, &db_inf);
                        let Some(srid) = db_inf.calc_srid(&id2, 0, self.default_srid) else {
                            continue;
                        };
                        db_inf.srid = srid;
                        update_auto_fields(&id2, &mut db_inf, auto_tables);
                        specs.insert(id2, SourceSpec::Table(db_inf));
                    }
                }
            }
        }

        Ok(())
    }

    /// Catalog query + config merge + auto-publish + id resolution for functions, inserting a [`SourceSpec::Function`] per id.
    /// A function's SQL is already known at catalog time, so the spec carries it directly.
    async fn discover_functions(
        &self,
        specs: &mut BTreeMap<String, SourceSpec>,
        warnings: &mut Vec<TileSourceWarning>,
    ) -> PostgresResult<()> {
        let mut db_funcs_info = query_available_function(&self.pool).await?;

        // Match configured function sources against the discovered catalog.
        let mut used = HashSet::<(String, String)>::new();
        for (id, cfg_inf) in &self.functions {
            match Self::build_one_function_info(&db_funcs_info, id, cfg_inf) {
                Ok((merged_inf, pg_sql_info)) => {
                    if !used.insert((cfg_inf.schema.clone(), cfg_inf.function.clone())) {
                        warn!(
                            source.id = %id,
                            schema = %cfg_inf.schema,
                            function = %cfg_inf.function,
                            "Configured duplicate source: multiple config entries resolve to the same function"
                        );
                    }
                    let id2 = self.resolve_id(id, &merged_inf);
                    warn_on_rename(id, &id2, "Function");
                    specs.insert(id2, SourceSpec::Function(merged_inf, pg_sql_info));
                }
                Err(error) => warnings.push(TileSourceWarning::SourceError {
                    source_id: id.clone(),
                    error,
                }),
            }
        }

        // Auto-publish remaining functions, sorted for deterministic id resolution.
        if let Some(auto_funcs) = &self.auto_functions {
            let schemas = auto_funcs
                .schemas
                .clone()
                .unwrap_or_else(|| db_funcs_info.keys().cloned().collect());
            info!(
                schemas = %schemas.iter().sorted().join(", "),
                source_id_format = %auto_funcs.source_id_format,
                "Auto-publishing functions"
            );
            for schema in schemas.iter().sorted() {
                let Some(schema) = normalize_key(&db_funcs_info, schema, "schema", "") else {
                    continue;
                };
                let db_funcs = db_funcs_info
                    .remove(&schema)
                    .expect("schema should be present in db_funcs_info after normalize_key lookup");
                for (func, (pg_sql, db_inf)) in db_funcs.into_iter().sorted_by(by_key) {
                    if used.contains(&(schema.clone(), func.clone())) {
                        continue;
                    }
                    let source_id = auto_funcs
                        .source_id_format
                        .replace("{schema}", &schema)
                        .replace("{function}", &func);
                    let id2 = self.resolve_id(&source_id, &db_inf);
                    specs.insert(id2, SourceSpec::Function(db_inf, pg_sql));
                }
            }
        }

        Ok(())
    }

    /// Turns one [`SourceSpec`] into a running [`PostgresSource`].
    ///
    /// The slow half of discovery: a table builds its SQL and computes its bounds here (the work [`discover`](Self::discover) deferred); a function is cheap, as its SQL is already known.
    /// The returned [`SourceSpec`] carries any computed bounds back for `--save-config`.
    pub async fn instantiate(
        &self,
        id: &str,
        spec: SourceSpec,
    ) -> PostgresResult<(BoxedSource, SourceSpec)> {
        match spec {
            SourceSpec::Table(info) => {
                let (id, pg_sql, info) = table_to_query(
                    id.to_string(),
                    info,
                    self.pool.clone(),
                    self.auto_bounds,
                    self.max_feature_count,
                )
                .await?;
                trace!(source.id = %id, sql = %pg_sql.sql_query, "source SQL query");
                let cache = info.cache.unwrap_or_default();
                let source = self.build_source(id, &info, pg_sql, cache);
                Ok((source, SourceSpec::Table(info)))
            }
            SourceSpec::Function(info, pg_sql) => {
                trace!(source.id = %id, sql = %pg_sql.sql_query, "source SQL query");
                let cache = info.cache.unwrap_or_default();
                let source = self.build_source(id.to_string(), &info, pg_sql.clone(), cache);
                Ok((source, SourceSpec::Function(info, pg_sql)))
            }
        }
    }

    /// Builds and returns a `TableInfo` generated by:
    ///
    /// a) Finding the `TableInfo` instance in the discovered tables map `table_infos_from_db` that
    ///    matches the (schema, table, `geometry_column`) specified in the input `table_info_from_config`'s values.
    /// b) Merging the result of the lookup with the values in `table_info_from_config`, giving `table_info_from_config` preference.
    ///
    /// If the given (schema, table, `geometry_column`) combination is not found, returns Err.
    fn build_one_table_info(
        &self,
        table_infos_from_db: &BTreeMap<String, BTreeMap<String, BTreeMap<String, TableInfo>>>,
        id: &String,
        table_info_from_config: &TableInfo,
    ) -> Result<TableInfo, String> {
        let table_infos_for_schema = find_info(
            table_infos_from_db,
            &table_info_from_config.schema,
            "schema",
            id,
        )?;
        let table_infos_for_table = find_info(
            table_infos_for_schema,
            &table_info_from_config.table,
            "table",
            id,
        )?;
        let table_info_for_geometry_column = find_info(
            table_infos_for_table,
            &table_info_from_config.geometry_column,
            "geometry column",
            id,
        )?;
        let merged_table_info = table_info_for_geometry_column
            .append_cfg_info(table_info_from_config, id, self.default_srid)
            .ok_or_else(|| format!("Failed to merge config info for table {id}"))?;
        Ok(merged_table_info)
    }

    /// Builds and returns a `FunctionInfo` generated by:
    ///
    /// a) Finding the `FunctionInfo` instance in the discovered functions map `function_infos_from_db` that
    ///    matches the (schema, function) values specified in the input `function_info_from_config`'s values.
    /// b) Merging the result of the lookup with the values in `function_info_from_config`, giving `function_info_from_config` preference.
    ///
    /// If the given (schema, function) combination is not found, returns Err.
    fn build_one_function_info(
        function_infos_from_db: &BTreeMap<
            String,
            BTreeMap<String, (PostgresSqlInfo, FunctionInfo)>,
        >,
        id: &str,
        function_info_from_config: &FunctionInfo,
    ) -> Result<(FunctionInfo, PostgresSqlInfo), String> {
        let function_infos_for_schema = find_info(
            function_infos_from_db,
            &function_info_from_config.schema,
            "schema",
            id,
        )?;
        if function_infos_for_schema.is_empty() {
            return Err(format!(
                "No functions found in schema {}. Only functions like (z,x,y) -> bytea and similar are considered. See README.md",
                function_info_from_config.schema
            ));
        }
        let function_name = &function_info_from_config.function;
        let (function_sql_info, table_info_from_schema) =
            find_info(function_infos_for_schema, function_name, "function", id)?;
        let merged_function_info =
            table_info_from_schema.append_cfg_info(function_info_from_config);
        Ok((merged_function_info, function_sql_info.clone()))
    }

    fn resolve_id<T: PostgresInfo>(&self, id: &str, src_inf: &T) -> String {
        let signature = format!("{}.{}", self.pool.get_id(), src_inf.format_id());
        self.id_resolver.resolve(id, signature)
    }

    /// Constructs a [`PostgresSource`] from a resolved source description and its SQL.
    /// The given `cache` falls back to the builder's default policy.
    fn build_source(
        &self,
        id: String,
        pg_info: &impl PostgresInfo,
        sql_info: PostgresSqlInfo,
        cache: CachePolicy,
    ) -> BoxedSource {
        let tilejson = pg_info.to_tilejson(id.clone());
        let tile_info = pg_info.tile_info();
        let cache = cache.or(self.default_cache);
        Box::new(PostgresSource::new(
            id,
            sql_info,
            tilejson,
            self.pool.clone(),
            tile_info,
            cache.zoom(),
        ))
    }

    fn configured_tables(&self) -> HashSet<(String, String)> {
        self.tables
            .values()
            .map(|t| (t.schema.to_lowercase(), t.table.to_lowercase()))
            .collect()
    }
}

fn update_auto_fields(
    id: &str,
    inf: &mut TableInfo,
    auto_tables: &PostgresAutoDiscoveryBuilderTables,
) {
    if inf.clip_geom.is_none() {
        inf.clip_geom = auto_tables.clip_geom;
    }
    if inf.buffer.is_none() {
        inf.buffer = auto_tables.buffer;
    }
    if inf.extent.is_none() {
        inf.extent = auto_tables.extent;
    }

    // Try to find any ID column in a list of table columns (properties) that match one of the given `id_column` values.
    // If found, modify `id_column` value on the table info.
    let Some(props) = inf.properties.as_mut() else {
        return;
    };
    let Some(try_columns) = &auto_tables.id_columns else {
        return;
    };

    for key in try_columns {
        let (column, typ) = if let Some(typ) = props.get(key) {
            (key, typ)
        } else {
            match find_kv_ignore_case(props, key) {
                Ok(Some(result)) => {
                    info!(
                        source.id = %id,
                        id_column.requested = %key,
                        id_column.found = %result,
                        "id_column not found by exact name, using case-insensitive match"
                    );
                    (result, props.get(result).expect("result key should be present in props after find_kv_ignore_case lookup"))
                }
                Ok(None) => continue,
                Err(multiple) => {
                    error!(
                        source.id = %id,
                        id_column.requested = %key,
                        id_column.candidates = %multiple.join(", "),
                        "Unable to configure source: id_column has no exact match or more than one potential match"
                    );
                    continue;
                }
            }
        };
        // ID column can be any integer type as defined in
        // https://github.com/postgis/postgis/blob/559c95d85564fb74fa9e3b7eafb74851810610da/postgis/mvt.c#L387C4-L387C66
        if typ != "int4" && typ != "int8" && typ != "int2" {
            warn!(
                schema = %inf.schema,
                table = %inf.table,
                column = %column,
                column.type = %typ,
                "Unable to use column as a tile feature ID because it has a non-integer type"
            );
            continue;
        }

        inf.id_column = Some(column.clone());
        let mut final_props = props.clone();
        final_props.remove(column);
        inf.properties = Some(final_props);
        return;
    }

    info!(
        schema = %inf.schema,
        table = %inf.table,
        searched = %try_columns.join(", "),
        "No ID column found for table - searched for an integer column"
    );
}

fn calc_auto(
    config: &PostgresConfig,
) -> (
    Option<PostgresAutoDiscoveryBuilderTables>,
    Option<PostgresAutoDiscoveryBuilderFunctions>,
) {
    let auto_tables = if use_auto_publish(config, false) {
        let schemas = get_auto_schemas!(config, tables);
        let bld = if let Object(PostgresCfgPublish {
            tables: Object(v), ..
        }) = &config.auto_publish
        {
            PostgresAutoDiscoveryBuilderTables {
                schemas,
                source_id_format: v
                    .source_id_format
                    .as_deref()
                    .unwrap_or("{table}")
                    .to_string(),
                id_columns: v.id_columns.opt_iter().map(|v| v.cloned().collect()),
                clip_geom: v.clip_geom,
                buffer: v.buffer,
                extent: v.extent,
            }
        } else {
            PostgresAutoDiscoveryBuilderTables {
                schemas,
                source_id_format: "{table}".to_string(),
                ..Default::default()
            }
        };
        Some(bld)
    } else {
        None
    };

    let auto_functions = if use_auto_publish(config, true) {
        Some(PostgresAutoDiscoveryBuilderFunctions {
            schemas: get_auto_schemas!(config, functions),
            source_id_format: if let Object(PostgresCfgPublish {
                functions:
                    Object(PostgresCfgPublishFuncs {
                        source_id_format: Some(v),
                        ..
                    }),
                ..
            }) = &config.auto_publish
            {
                v.clone()
            } else {
                "{function}".to_string()
            },
        })
    } else {
        None
    };

    (auto_tables, auto_functions)
}

fn use_auto_publish(config: &PostgresConfig, for_functions: bool) -> bool {
    match &config.auto_publish {
        NoValue => config.tables.is_none() && config.functions.is_none(),
        Object(funcs) => {
            if for_functions {
                // If auto_publish.functions is set, and currently asking for .tables which is missing,
                // .tables becomes the inverse of functions (i.e. an obj or true in tables means false in functions)
                match &funcs.functions {
                    NoValue => matches!(funcs.tables, NoValue | Bool(false)),
                    Object(_) => true,
                    Bool(v) => *v,
                }
            } else {
                match &funcs.tables {
                    NoValue => matches!(funcs.functions, NoValue | Bool(false)),
                    Object(_) => true,
                    Bool(v) => *v,
                }
            }
        }
        Bool(v) => *v,
    }
}

fn warn_on_rename(old_id: &String, new_id: &String, typ: &str) {
    if old_id != new_id {
        warn!(
            source.kind = %typ,
            source.id.old = %old_id,
            source.id.new = %new_id,
            "source was renamed due to ID conflict"
        );
    }
}

/// A comparator for sorting tuples by first element
fn by_key<T>(a: &(String, T), b: &(String, T)) -> Ordering {
    a.0.cmp(&b.0)
}

#[cfg(all(test, feature = "test-pg"))]
#[expect(clippy::unwrap_used, clippy::panic)]
mod tests {
    use backon::{ConstantBuilder, Retryable as _};
    use indoc::indoc;
    use insta::{assert_debug_snapshot, assert_yaml_snapshot};
    use rstest::rstest;
    use testcontainers_modules::postgres::Postgres;
    use testcontainers_modules::testcontainers::ImageExt as _;
    use testcontainers_modules::testcontainers::runners::AsyncRunner as _;

    use super::*;

    async fn start_old_postgis_container()
    -> testcontainers_modules::testcontainers::ContainerAsync<Postgres> {
        const MAX_START_ATTEMPTS: usize = 3;
        const RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

        (|| async {
            Postgres::default()
                .with_name("postgis/postgis")
                .with_tag("11-3.0") // purposely very old and stable
                .start()
                .await
        })
        .retry(
            ConstantBuilder::default()
                .with_delay(RETRY_DELAY)
                .with_max_times(MAX_START_ATTEMPTS),
        )
        .sleep(tokio::time::sleep)
        .await
        .unwrap_or_else(|e| {
            panic!("failed to launch container after {MAX_START_ATTEMPTS} attempts: {e}")
        })
    }

    #[derive(serde::Serialize)]
    struct AutoCfg {
        auto_table: Option<PostgresAutoDiscoveryBuilderTables>,
        auto_funcs: Option<PostgresAutoDiscoveryBuilderFunctions>,
    }
    fn auto(content: &str) -> AutoCfg {
        let cfg: PostgresConfig = serde_yaml::from_str(content).unwrap();
        let (auto_table, auto_funcs) = calc_auto(&cfg);
        AutoCfg {
            auto_table,
            auto_funcs,
        }
    }

    #[rstest]
    #[case::empty_config("{}")]
    #[case::auto_publish_true("auto_publish: true")]
    fn auto_publish_defaults_to_both(#[case] config_yaml: &str) {
        insta::allow_duplicates! {
            assert_yaml_snapshot!(auto(config_yaml), @r#"
            auto_table:
              source_id_format: "{table}"
            auto_funcs:
              source_id_format: "{function}"
            "#);
        }
    }

    #[rstest]
    #[case::tables_listed("tables: {}")]
    #[case::functions_listed("functions: {}")]
    #[case::auto_publish_false("auto_publish: false")]
    fn auto_publish_disabled(#[case] config_yaml: &str) {
        insta::allow_duplicates! {
            assert_yaml_snapshot!(auto(config_yaml), @r"
            auto_table: ~
            auto_funcs: ~
            ");
        }
    }

    #[rstest]
    #[case::tables_on(indoc! {"
        auto_publish:
            from_schemas: public
            tables: true"})]
    #[case::functions_off(indoc! {"
        auto_publish:
            from_schemas: public
            functions: false"})]
    fn auto_publish_tables_only(#[case] config_yaml: &str) {
        insta::allow_duplicates! {
            assert_yaml_snapshot!(auto(config_yaml), @r#"
            auto_table:
              schemas:
                - public
              source_id_format: "{table}"
            auto_funcs: ~
            "#);
        }
    }

    #[rstest]
    #[case::functions_on(indoc! {"
        auto_publish:
            from_schemas: public
            functions: true"})]
    #[case::tables_off(indoc! {"
        auto_publish:
            from_schemas: public
            tables: false"})]
    fn auto_publish_functions_only(#[case] config_yaml: &str) {
        insta::allow_duplicates! {
            assert_yaml_snapshot!(auto(config_yaml), @r#"
            auto_table: ~
            auto_funcs:
              schemas:
                - public
              source_id_format: "{function}"
            "#);
        }
    }

    #[test]
    fn auto_publish_merges_from_schemas_with_id_format() {
        let cfg = auto(indoc! {"
            auto_publish:
                from_schemas: public
                tables:
                    from_schemas: osm
                    id_format: 'foo_{schema}.{table}_bar'"});
        assert_yaml_snapshot!(cfg,
        {
            ".auto_table.schemas" => insta::sorted_redaction()
        },
        @r#"
        auto_table:
          schemas:
            - osm
            - public
          source_id_format: "foo_{schema}.{table}_bar"
        auto_funcs: ~
        "#);
    }

    #[test]
    fn auto_publish_merges_from_schemas_with_source_id_format() {
        let cfg = auto(indoc! {"
            auto_publish:
                from_schemas: public
                tables:
                    from_schemas: osm
                    source_id_format: '{schema}.{table}'"});
        assert_yaml_snapshot!(cfg,
          {
              ".auto_table.schemas" => insta::sorted_redaction()
          },
          @r#"
        auto_table:
          schemas:
            - osm
            - public
          source_id_format: "{schema}.{table}"
        auto_funcs: ~
        "#);
    }

    #[test]
    fn auto_publish_accepts_from_schemas_list() {
        let cfg = auto(indoc! {"
            auto_publish:
                tables:
                    from_schemas:
                      - osm
                      - public"});
        assert_yaml_snapshot!(cfg,
          {
              ".auto_table.schemas" => insta::sorted_redaction()
          },
          @r#"
        auto_table:
          schemas:
            - osm
            - public
          source_id_format: "{table}"
        auto_funcs: ~
        "#);
    }

    /// Seeds the database behind `builder` with arbitrary setup SQL.
    async fn seed(builder: &PostgresAutoDiscoveryBuilder, sql: &str) {
        builder
            .pool
            .get()
            .await
            .unwrap()
            .batch_execute(sql)
            .await
            .unwrap();
    }

    async fn builder_for(
        config_yaml: &str,
    ) -> (
        PostgresAutoDiscoveryBuilder,
        testcontainers_modules::testcontainers::ContainerAsync<Postgres>,
    ) {
        let container = start_old_postgis_container().await;
        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();
        let connection_string =
            format!("postgres://postgres:postgres@{host}:{port}/postgres?sslmode=disable");

        let mut config: PostgresConfig = serde_yaml::from_str(config_yaml).unwrap();
        config.connection_string = Some(connection_string);

        let builder = PostgresAutoDiscoveryBuilder::new(
            &config,
            IdResolver::default(),
            CachePolicy::default(),
        )
        .await
        .expect("Failed to create builder");
        (builder, container)
    }

    #[tokio::test]
    async fn discover_and_instantiate_table() {
        let (builder, _container) = builder_for(indoc! {r"
            tables:
              my_points:
                schema: public
                table: points
                geometry_column: geom
                srid: 4326
                geometry_type: POINT
        "})
        .await;
        seed(
            &builder,
            "CREATE TABLE public.points (gid serial PRIMARY KEY, geom geometry(Point, 4326));\
             INSERT INTO public.points (geom) VALUES (ST_SetSRID(ST_MakePoint(1, 2), 4326));",
        )
        .await;

        let (mut specs, warnings) = builder.discover().await.expect("discover failed");
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        assert_eq!(specs.len(), 1);

        let SourceSpec::Table(info) = specs.get("my_points").expect("spec for my_points") else {
            panic!("expected a Table spec");
        };
        // discover defers the slow bounds job, so the spec has none yet; the rest is the merged catalog+config metadata.
        assert_eq!(info.bounds, None);
        assert_yaml_snapshot!(info, @"
        schema: public
        table: points
        srid: 4326
        geometry_column: geom
        geometry_type: POINT
        ");

        let spec = specs.remove("my_points").expect("spec for my_points");
        let (source, spec) = builder
            .instantiate("my_points", spec)
            .await
            .expect("instantiate failed");

        assert_eq!(source.get_id(), "my_points");
        // instantiate runs the bounds calculation that discover deferred.
        let SourceSpec::Table(info) = spec else {
            panic!("expected a Table spec back");
        };
        assert!(
            info.bounds.is_some(),
            "instantiate must run the deferred bounds calculation"
        );
    }

    #[tokio::test]
    async fn discover_auto_publishes_from_catalog_and_is_rerunnable() {
        let (builder, _container) = builder_for("{}").await;
        seed(
            &builder,
            "CREATE TABLE public.roads (gid serial PRIMARY KEY, geom geometry(LineString, 4326));",
        )
        .await;
        seed(&builder, TILE_FUNCTION_SQL).await;

        let (first, warnings) = builder.discover().await.expect("first discover failed");
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");

        // The table is auto-published under the default `{table}` id, with its columns read from the catalog.
        let SourceSpec::Table(info) = first.get("roads").expect("spec for roads") else {
            panic!("expected a Table spec");
        };
        assert_eq!(info.bounds, None);
        assert_yaml_snapshot!(info, @"
        schema: public
        table: roads
        srid: 4326
        geometry_column: geom
        geometry_type: LINESTRING
        properties:
          gid: int4
        ");
        // The function is auto-published too, under the default `{function}` id.
        assert!(
            matches!(first.get("my_func"), Some(SourceSpec::Function(..))),
            "expected an auto-published function spec for my_func"
        );

        // An idle re-discover must return the same ids, so a future Reloader sees "no change".
        let (second, _) = builder.discover().await.expect("second discover failed");
        let first_ids: Vec<&String> = first.keys().collect();
        let second_ids: Vec<&String> = second.keys().collect();
        assert_eq!(first_ids, second_ids, "discover must return stable ids");
    }

    const TILE_FUNCTION_SQL: &str = "CREATE FUNCTION public.my_func(z integer, x integer, y integer) \
         RETURNS bytea AS $$ SELECT NULL::bytea $$ LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE;";

    #[tokio::test]
    async fn discover_and_instantiate_function() {
        let (builder, _container) = builder_for(indoc! {r"
            functions:
              my_func:
                schema: public
                function: my_func
        "})
        .await;
        seed(&builder, TILE_FUNCTION_SQL).await;

        let (mut specs, warnings) = builder.discover().await.expect("discover failed");
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");

        let SourceSpec::Function(info, sql) = specs.get("my_func").expect("spec for my_func")
        else {
            panic!("expected a Function spec");
        };
        assert_yaml_snapshot!(info, @"
        schema: public
        function: my_func
        ");
        // a function's SQL is already known at catalog time
        assert_debug_snapshot!(sql, @r#"
        PostgresSqlInfo {
            sql_query: "SELECT \"public\".\"my_func\"($1::integer, $2::integer, $3::integer) AS tile",
            use_url_query: false,
            signature: "public.my_func(integer, integer, integer) -> bytea",
        }
        "#);

        let spec = specs.remove("my_func").expect("spec for my_func");
        let (source, _) = builder
            .instantiate("my_func", spec)
            .await
            .expect("instantiate failed");
        assert_eq!(source.get_id(), "my_func");
    }

    #[tokio::test]
    async fn instantiate_failure_surfaces_as_error() {
        let (builder, _container) = builder_for(indoc! {r"
            tables:
              my_points:
                schema: public
                table: points
                geometry_column: geom
                srid: 4326
                geometry_type: POINT
        "})
        .await;
        seed(
            &builder,
            "CREATE TABLE public.points (gid serial PRIMARY KEY, geom geometry(Point, 4326));",
        )
        .await;

        let (mut specs, _) = builder.discover().await.expect("discover failed");
        let spec = specs.remove("my_points").expect("spec for my_points");

        // The table vanishes between discover and instantiate.
        seed(&builder, "DROP TABLE public.points;").await;

        let result = builder.instantiate("my_points", spec).await;
        assert!(
            result.is_err(),
            "instantiating a vanished table must surface an error, not be silently dropped"
        );
    }

    #[tokio::test]
    async fn discover_missing_sources_surface_as_warnings() {
        let (builder, _container) = builder_for(indoc! {r"
            tables:
              nonexistent_table:
                schema: public
                table: this_table_does_not_exist
                geometry_column: geom
                srid: 4326
                geometry_type: POINT

            functions:
              nonexistent_function:
                schema: public
                function: this_function_does_not_exist
        "})
        .await;

        let (specs, warnings) = builder.discover().await.expect("discover failed");

        // Neither the missing table nor the missing function can be resolved, so no spec is produced and each surfaces as its own warning.
        assert!(specs.is_empty(), "unexpected specs: {specs:?}");

        let warned_ids: HashSet<&str> = warnings
            .iter()
            .map(|w| match w {
                TileSourceWarning::SourceError { source_id, .. } => source_id.as_str(),
                TileSourceWarning::PathError { .. } => {
                    panic!("Expected SourceError warning, got: {w:?}")
                }
            })
            .collect();
        assert_eq!(
            warned_ids,
            HashSet::from(["nonexistent_table", "nonexistent_function"]),
        );
    }
}
