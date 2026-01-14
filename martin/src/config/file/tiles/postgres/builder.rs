use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

use futures::future::join_all;
use itertools::Itertools as _;
use tracing::{debug, error, info, warn};
use martin_core::config::IdResolver;
use martin_core::config::OptBoolObj::{Bool, NoValue, Object};
use martin_core::config::OptOneMany::NoVals;
use martin_core::tiles::BoxedSource;
use martin_core::tiles::postgres::{
    PostgresError, PostgresPool, PostgresResult, PostgresSource, PostgresSqlInfo,
};

use crate::config::args::BoundsCalcType;
use crate::config::file::postgres::resolver::{
    query_available_function, query_available_tables, table_to_query,
};
use crate::config::file::postgres::utils::{find_info, find_kv_ignore_case, normalize_key};
use crate::config::file::postgres::{
    FuncInfoSources, FunctionInfo, POOL_SIZE_DEFAULT, PostgresCfgPublish, PostgresCfgPublishFuncs,
    PostgresConfig, PostgresInfo, TableInfo, TableInfoSources,
};
use crate::config::file::{ConfigFileError, ConfigFileResult, TileSourceWarning};

/// Builder for [`PostgresSource`]' auto-discovery of functions and tables.
#[derive(Debug)]
pub struct PostgresAutoDiscoveryBuilder {
    pool: PostgresPool,
    /// If a spatial table has SRID 0, then this SRID will be used as a fallback
    default_srid: Option<i32>,
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
    extent: Option<u32>,
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
    pub async fn new(config: &PostgresConfig, id_resolver: IdResolver) -> ConfigFileResult<Self> {
        let pool = PostgresPool::new(
            config.connection_string.as_ref().unwrap().as_str(),
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

    /// Discovers and instantiates table-based tile sources.
    pub async fn instantiate_tables(
        &self,
    ) -> PostgresResult<(Vec<BoxedSource>, TableInfoSources, Vec<TileSourceWarning>)> {
        let restrict_to_tables = if self.auto_tables.is_none() {
            Some(self.configured_tables())
        } else {
            None
        };

        let mut db_tables_info = query_available_tables(&self.pool, restrict_to_tables).await?;
        let mut warnings = Vec::new();

        // Match configured sources with the discovered ones and add them to the pending list.
        let mut used = HashSet::<(&str, &str, &str)>::new();
        let mut pending = Vec::new();
        for (id, cfg_inf) in &self.tables {
            // TODO: move this validation to serde somehow?
            if cfg_inf.extent == Some(0) {
                return Err(PostgresError::InvalidTableExtent(
                    id.clone(),
                    cfg_inf.format_id(),
                ));
            }

            match self.build_one_table_info(&db_tables_info, id, cfg_inf) {
                Ok(merged_inf) => {
                    let dup =
                        !used.insert((&cfg_inf.schema, &cfg_inf.table, &cfg_inf.geometry_column));
                    let dup = if dup { "duplicate " } else { "" };

                    let id2 = self.resolve_id(id, &merged_inf);
                    warn_on_rename(id, &id2, "Table");
                    info!("Configured {dup}source {id2} from {}", summary(&merged_inf));
                    pending.push(table_to_query(
                        id2,
                        merged_inf,
                        self.pool.clone(),
                        self.auto_bounds,
                        self.max_feature_count,
                    ));
                }
                Err(error) => warnings.push(TileSourceWarning::SourceError {
                    source_id: id.clone(),
                    error,
                }),
            }
        }

        // Sort the discovered sources by schema, table and geometry column to ensure a consistent behavior
        if let Some(auto_tables) = &self.auto_tables {
            let schemas = auto_tables
                .schemas
                .clone()
                .unwrap_or_else(|| db_tables_info.keys().cloned().collect());
            info!(
                "Auto-publishing tables in schemas [{}] as '{}' sources",
                schemas.iter().sorted().join(", "),
                auto_tables.source_id_format,
            );

            for schema in schemas.iter().sorted() {
                let Some(schema) = normalize_key(&db_tables_info, schema, "schema", "") else {
                    continue;
                };
                let db_tables = db_tables_info.remove(&schema).unwrap();
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
                        info!("Discovered source {id2} from {}", summary(&db_inf));
                        pending.push(table_to_query(
                            id2,
                            db_inf,
                            self.pool.clone(),
                            self.auto_bounds,
                            self.max_feature_count,
                        ));
                    }
                }
            }
        }

        let mut res = Vec::new();
        let mut info_map = TableInfoSources::new();
        let pending = join_all(pending).await;
        for src in pending {
            match src {
                Err(v) => {
                    error!("Failed to create a source: {v}");
                }
                Ok((id, pg_sql, src_inf)) => {
                    debug!("{id} query: {}", pg_sql.sql_query);
                    self.add_func_src(&mut res, id.clone(), &src_inf, pg_sql.clone());
                    info_map.insert(id, src_inf);
                }
            }
        }

        Ok((res, info_map, warnings))
    }

    /// Discovers and instantiates function-based tile sources.
    pub async fn instantiate_functions(
        &self,
    ) -> PostgresResult<(Vec<BoxedSource>, FuncInfoSources, Vec<TileSourceWarning>)> {
        let mut db_funcs_info = query_available_function(&self.pool).await?;
        let mut warnings = Vec::new();
        let mut res = Vec::new();
        let mut info_map = FuncInfoSources::new();
        let mut used = HashSet::<(&str, &str)>::new();

        for (id, cfg_inf) in &self.functions {
            match Self::build_one_function_info(&db_funcs_info, id, cfg_inf) {
                Ok((merged_inf, pg_sql_info)) => {
                    let dup = !used.insert((&cfg_inf.schema, &cfg_inf.function));
                    let dup = if dup { "duplicate " } else { "" };
                    let id2 = self.resolve_id(id, &merged_inf);
                    self.add_func_src(&mut res, id2.clone(), &merged_inf, pg_sql_info.clone());
                    warn_on_rename(id, &id2, "Function");
                    let signature = &pg_sql_info.signature;
                    info!("Configured {dup}source {id2} from the function {signature}");
                    debug!("{id2} query: {}", pg_sql_info.sql_query);
                    info_map.insert(id2, merged_inf);
                }
                Err(error) => {
                    warnings.push(TileSourceWarning::SourceError {
                        source_id: id.clone(),
                        error,
                    });
                }
            }
        }

        // Sort the discovered sources by schema and function name to ensure a consistent behavior
        if let Some(auto_funcs) = &self.auto_functions {
            let schemas = auto_funcs
                .schemas
                .clone()
                .unwrap_or_else(|| db_funcs_info.keys().cloned().collect());
            info!(
                "Auto-publishing functions in schemas [{}] as '{}' sources",
                schemas.iter().sorted().join(", "),
                auto_funcs.source_id_format,
            );

            for schema in schemas.iter().sorted() {
                let Some(schema) = normalize_key(&db_funcs_info, schema, "schema", "") else {
                    continue;
                };
                let db_funcs = db_funcs_info.remove(&schema).unwrap();
                for (func, (pg_sql, db_inf)) in db_funcs.into_iter().sorted_by(by_key) {
                    if used.contains(&(schema.as_str(), func.as_str())) {
                        continue;
                    }
                    let source_id = auto_funcs
                        .source_id_format
                        .replace("{schema}", &schema)
                        .replace("{function}", &func);
                    let id2 = self.resolve_id(&source_id, &db_inf);
                    self.add_func_src(&mut res, id2.clone(), &db_inf, pg_sql.clone());
                    info!("Discovered source {id2} from function {}", pg_sql.signature);
                    debug!("{id2} query: {}", pg_sql.sql_query);
                    info_map.insert(id2, db_inf);
                }
            }
        }
        Ok((res, info_map, warnings))
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

    fn add_func_src(
        &self,
        sources: &mut Vec<BoxedSource>,
        id: String,
        pg_info: &impl PostgresInfo,
        sql_info: PostgresSqlInfo,
    ) {
        let tilejson = pg_info.to_tilejson(id.clone());
        let source = PostgresSource::new(id, sql_info, tilejson, self.pool.clone());
        sources.push(Box::new(source));
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
                        "For source {id}, id_column '{key}' was not found, but found '{result}' instead."
                    );
                    (result, props.get(result).unwrap())
                }
                Ok(None) => continue,
                Err(multiple) => {
                    error!(
                        "Unable to configure source {id} because id_column '{key}' has no exact match or more than one potential matches: {}",
                        multiple.join(", ")
                    );
                    continue;
                }
            }
        };
        // ID column can be any integer type as defined in
        // https://github.com/postgis/postgis/blob/559c95d85564fb74fa9e3b7eafb74851810610da/postgis/mvt.c#L387C4-L387C66
        if typ != "int4" && typ != "int8" && typ != "int2" {
            warn!(
                "Unable to use column `{key}` in table {}.{} as a tile feature ID because it has a non-integer type `{typ}`.",
                inf.schema, inf.table
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
        "No ID column found for table {}.{} - searched for an integer column named {}.",
        inf.schema,
        inf.table,
        try_columns.join(", ")
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
        warn!("{typ} source {old_id} was renamed to {new_id} due to ID conflict");
    }
}

fn summary(info: &TableInfo) -> String {
    let relkind: Cow<_> = match info.relkind {
        Some('v') => "view".into(),
        Some('m') => "materialized view".into(),
        Some('r') => "table".into(),
        // printing these variants is likely a bug
        Some(r) => format!("unknown relkind={r}").into(),
        None => "unknown relkind".into(),
    };
    let id: Cow<_> = info.id_column.as_ref().map_or_else(
        || "no ID column".into(),
        |id| format!("ID column {id}").into(),
    );
    format!(
        "{relkind} {}.{} with {} column ({}, SRID={}), {id}",
        info.schema,
        info.table,
        info.geometry_column,
        info.geometry_type
            .as_deref()
            .unwrap_or("UNKNOWN GEOMETRY TYPE"),
        info.srid,
    )
}

/// A comparator for sorting tuples by first element
fn by_key<T>(a: &(String, T), b: &(String, T)) -> Ordering {
    a.0.cmp(&b.0)
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use insta::assert_yaml_snapshot;

    use super::*;

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

    #[test]
    #[expect(clippy::too_many_lines)]
    fn test_auto_publish_no_auto() {
        let cfg = auto("{}");
        assert_yaml_snapshot!(cfg, @r#"
        auto_table:
          source_id_format: "{table}"
        auto_funcs:
          source_id_format: "{function}"
        "#);

        let cfg = auto("tables: {}");
        assert_yaml_snapshot!(cfg, @r"
        auto_table: ~
        auto_funcs: ~
        ");

        let cfg = auto("functions: {}");
        assert_yaml_snapshot!(cfg, @r"
        auto_table: ~
        auto_funcs: ~
        ");

        let cfg = auto("auto_publish: true");
        assert_yaml_snapshot!(cfg, @r#"
        auto_table:
          source_id_format: "{table}"
        auto_funcs:
          source_id_format: "{function}"
        "#);

        let cfg = auto("auto_publish: false");
        assert_yaml_snapshot!(cfg, @r"
        auto_table: ~
        auto_funcs: ~
        ");

        let cfg = auto(indoc! {"
            auto_publish:
                from_schemas: public
                tables: true"});
        assert_yaml_snapshot!(cfg, @r#"
        auto_table:
          schemas:
            - public
          source_id_format: "{table}"
        auto_funcs: ~
        "#);

        let cfg = auto(indoc! {"
            auto_publish:
                from_schemas: public
                functions: true"});
        assert_yaml_snapshot!(cfg, @r#"
        auto_table: ~
        auto_funcs:
          schemas:
            - public
          source_id_format: "{function}"
        "#);

        let cfg = auto(indoc! {"
            auto_publish:
                from_schemas: public
                tables: false"});
        assert_yaml_snapshot!(cfg, @r#"
        auto_table: ~
        auto_funcs:
          schemas:
            - public
          source_id_format: "{function}"
        "#);

        let cfg = auto(indoc! {"
            auto_publish:
                from_schemas: public
                functions: false"});
        assert_yaml_snapshot!(cfg, @r#"
        auto_table:
          schemas:
            - public
          source_id_format: "{table}"
        auto_funcs: ~
        "#);

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

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_nonexistent_tables_functions_generate_warning() {
        use testcontainers_modules::postgres::Postgres;
        use testcontainers_modules::testcontainers::ImageExt;
        use testcontainers_modules::testcontainers::runners::AsyncRunner;

        let container = Postgres::default()
            .with_name("postgis/postgis")
            .with_tag("11-3.0") // purposely very old and stable
            .start()
            .await
            .expect("container launched");

        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();

        let connection_string =
            format!("postgres://postgres:postgres@{host}:{port}/postgres?sslmode=disable");

        let config_yaml = indoc! {r"
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
        "};

        let mut config: PostgresConfig = serde_yaml::from_str(config_yaml).unwrap();
        config.connection_string = Some(connection_string);

        let builder = PostgresAutoDiscoveryBuilder::new(&config, IdResolver::default())
            .await
            .expect("Failed to create builder");

        let (table_sources, _info_map, table_warnings) = builder
            .instantiate_tables()
            .await
            .expect("Failed to instantiate tables");

        assert_eq!(table_sources.len(), 0);
        assert_eq!(table_warnings.len(), 1);

        match &table_warnings[0] {
            TileSourceWarning::SourceError {
                source_id,
                error: _,
            } => {
                assert_eq!(source_id, "nonexistent_table");
            }
            TileSourceWarning::PathError { .. } => {
                panic!("Expected SourceError warning, got: {:?}", table_warnings[0])
            }
        }

        let (function_sources, _info_map, function_warnings) = builder
            .instantiate_functions()
            .await
            .expect("Failed to instantiate functions");

        assert_eq!(function_sources.len(), 0);
        assert_eq!(function_warnings.len(), 1);

        match &function_warnings[0] {
            TileSourceWarning::SourceError {
                source_id,
                error: _,
            } => {
                assert_eq!(source_id, "nonexistent_function");
            }
            TileSourceWarning::PathError { .. } => panic!(
                "Expected SourceError warning, got: {:?}",
                function_warnings[0]
            ),
        }
    }
}
