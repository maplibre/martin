use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use martin_core::tiles::postgres::PostgresError::{CannotTransform, PostgresError};
use martin_core::tiles::postgres::{PostgresPool, PostgresResult, PostgresSource, PostgresSqlInfo, PostgresTiler};
use martin_core::tiles::{BackendSource, BoxedSource};
use martin_tile_utils::{TileGrid, WEB_MERCATOR_QUAD_ID};
use tracing::{debug, error, info, trace, warn};

use crate::config::args::BoundsCalcType;
use crate::config::args::OrderByType;
use crate::config::file::postgres::auto_publish::{
    AutoPublish, Catalog, Decision, IdColumnOutcome, SkipReason, Transform,
};
use crate::config::file::postgres::resolver::{
    SqlFuncInfoMapMap, SqlTableInfoMapMapMap, query_available_function, query_available_tables, query_schemas,
    table_to_query,
};
use crate::config::file::postgres::utils::{find_info, find_schema_info};
use crate::config::file::postgres::{
    DEFAULT_POOL_SIZE, FuncInfoSources, FunctionInfo, PgTileGrid, PostgresConfig, PostgresInfo, SourceSpec, Srid,
    TableInfo, TableInfoSources,
};
use crate::config::file::{CachePolicy, ConfigFileError, ConfigFileResult, TileGrids, TileSourceWarning};
use crate::config::primitives::IdResolver;

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
    /// Try sorting features by their feature ID in ascending order.
    order_by: Option<OrderByType>,
    /// The grids this connection's sources may be served in, by name, each paired with its `PostGIS` SRID
    tile_grids: HashMap<String, PgTileGrid>,
    /// The grid for sources that do not name one
    default_tile_grid: String,
    auto: AutoPublish,
    id_resolver: IdResolver,
    /// Associative arrays of table sources
    tables: TableInfoSources,
    functions: FuncInfoSources,
}

/// Pairs a grid with the SRID `PostGIS` knows its CRS by, and notes whether a pole lies inside it.
///
/// `EPSG` codes are their own SRIDs.
/// Any other authority is looked up in `spatial_ref_sys`, where users register planetary and other non-EPSG systems.
async fn resolve_tile_grid(pool: &PostgresPool, grid: &TileGrid) -> ConfigFileResult<PgTileGrid> {
    if grid.is_web_mercator() {
        return Ok(PgTileGrid::web_mercator());
    }
    if grid.is_simple() {
        // plain planar units are PostGIS's "unknown" SRID, and never contain a pole
        return Ok(PgTileGrid::new(grid.clone(), 0));
    }
    let code: i32 = grid
        .crs_code()
        .parse()
        .map_err(|_not_an_integer| ConfigFileError::TileGridCrsCodeNotNumeric {
            grid: grid.id().to_owned(),
            crs: grid.crs().to_owned(),
        })?;
    let is_epsg = grid.crs_authority().eq_ignore_ascii_case("EPSG");
    let srid = if is_epsg {
        Srid::try_from(code)
            .map_err(|_out_of_range| ConfigFileError::TileGridCrsCodeNotSrid {
                grid: grid.id().to_owned(),
                crs: grid.crs().to_owned(),
            })?
            .get()
    } else {
        let conn = pool
            .get()
            .await
            .map_err(|e| ConfigFileError::TileGridResolution(Box::new(e), grid.id().to_owned()))?;
        conn.query_opt(
            "SELECT srid FROM spatial_ref_sys WHERE auth_name = $1 AND auth_srid = $2",
            &[&grid.crs_authority(), &code],
        )
        .await
        .map_err(|e| {
            ConfigFileError::TileGridResolution(
                Box::new(PostgresError(e, "looking up a tile grid's CRS in spatial_ref_sys")),
                grid.id().to_owned(),
            )
        })?
        .map(|row| row.get::<_, i32>("srid"))
        .ok_or_else(|| ConfigFileError::TileGridCrsNotInDatabase {
            grid: grid.id().to_owned(),
            crs: grid.crs().to_owned(),
        })?
    };
    let mut pg_grid = PgTileGrid::new(grid.clone(), srid);
    if is_epsg && pole_inside(pool, &pg_grid).await {
        pg_grid.mark_pole_inside();
    }
    Ok(pg_grid)
}

/// Whether the north or south pole lies inside the grid's zoom-0 tile.
///
/// Answered by the database, which knows the projection.
/// Any failure counts as "no", since this only feeds a warning.
async fn pole_inside(pool: &PostgresPool, grid: &PgTileGrid) -> bool {
    let [min_x, min_y, max_x, max_y] = grid.grid().bounds();
    let srid = grid.srid();
    let sql = "
SELECT ST_Contains(env, ST_Transform(ST_SetSRID(ST_MakePoint(0, 90), 4326), $5::integer))
    OR ST_Contains(env, ST_Transform(ST_SetSRID(ST_MakePoint(0, -90), 4326), $5::integer))
FROM ST_MakeEnvelope($1::float8, $2::float8, $3::float8, $4::float8, $5::integer) AS env";
    let Ok(conn) = pool.get().await else {
        return false;
    };
    match conn.query_one(sql, &[&min_x, &min_y, &max_x, &max_y, &srid]).await {
        Ok(row) => row.get::<_, Option<bool>>(0).unwrap_or(false),
        Err(e) => {
            debug!(
                tile_grid = grid.grid().id(),
                error = %e,
                "Could not check whether a pole lies inside the grid"
            );
            // a failed projection leaves PostGIS state behind that breaks later transforms on this connection
            PostgresPool::discard(conn);
            false
        }
    }
}

impl PostgresAutoDiscoveryBuilder {
    /// Creates a new `PostgreSQL` source builder from the [`PostgresConfig`].
    ///
    /// Duplicate names are deterministically converted to unique names.
    pub async fn new(
        config: &PostgresConfig,
        id_resolver: IdResolver,
        default_cache: CachePolicy,
        grids: &TileGrids,
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
            config.pool_size.unwrap_or(DEFAULT_POOL_SIZE).get(),
            config.retry_timeout.unwrap_or_default(),
        )
        .await
        .map_err(ConfigFileError::PostgresPoolCreationFailed)?;

        // Every grid a source of this connection can name, resolved against the database once.
        let default_tile_grid = config
            .tile_grid
            .clone()
            .unwrap_or_else(|| WEB_MERCATOR_QUAD_ID.to_owned());
        let connection = std::iter::once(("The connection's tile_grid".to_owned(), default_tile_grid.as_str()));
        let tables = config.tables.iter().flatten().filter_map(|(id, table)| {
            let grid = table.tile_grid.as_deref()?;
            Some((format!("Table source {id}"), grid))
        });
        let functions = config.functions.iter().flatten().filter_map(|(id, function)| {
            let grid = function.tile_grid.as_deref()?;
            Some((format!("Function source {id}"), grid))
        });
        let mut tile_grids = HashMap::new();
        for (what, name) in connection.chain(tables).chain(functions) {
            if tile_grids.contains_key(name) {
                continue;
            }
            let grid = grids.get(name).ok_or_else(|| ConfigFileError::UnknownTileGrid {
                what,
                grid: name.to_owned(),
                known: grids.names().join(", "),
            })?;
            tile_grids.insert(name.to_owned(), resolve_tile_grid(&pool, grid).await?);
        }

        #[cfg_attr(not(feature = "unstable-generate"), expect(unused_mut))]
        let mut tables = config.tables.clone().unwrap_or_default();
        #[cfg(feature = "unstable-generate")]
        for info in tables.values_mut() {
            info.prefetch = info.prefetch.or(config.prefetch);
        }

        Ok(Self {
            pool,
            default_srid: config.default_srid.map(Srid::get),
            default_cache,
            auto_bounds: config.auto_bounds.unwrap_or_default(),
            max_feature_count: config.max_feature_count,
            order_by: config.order_by,
            tile_grids,
            default_tile_grid,
            id_resolver,
            tables,
            functions: config.functions.clone().unwrap_or_default(),
            auto: AutoPublish::new(config),
        })
    }

    /// Returns the bounds calculation type for this builder.
    #[must_use]
    pub const fn auto_bounds(&self) -> BoundsCalcType {
        self.auto_bounds
    }

    #[cfg(feature = "unstable-generate")]
    #[must_use]
    pub fn pool(&self) -> &PostgresPool {
        &self.pool
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
    pub async fn discover(&self) -> PostgresResult<(BTreeMap<String, SourceSpec>, Vec<TileSourceWarning>)> {
        let mut specs = BTreeMap::new();
        let mut warnings = Vec::new();
        let all_schemas = query_schemas(&self.pool).await?;

        let reads = self.auto.reads();
        let mut catalog = Catalog::default();
        if let Some(tables) = reads.tables {
            catalog.tables = query_available_tables(&self.pool, tables.schemas, tables.only).await?;
        }
        if reads.functions {
            catalog.functions = query_available_function(&self.pool).await?;
        }
        let grid = self.tile_grid_for(None);
        let mut refusals = HashMap::new();
        for transform in self.auto.transforms(&catalog.tables, grid, self.default_srid) {
            match self.pool.check_transform(transform.grid, transform.table).await {
                Ok(()) => {}
                Err(refusal @ CannotTransform(..)) => {
                    refusals.insert(transform, refusal.to_string());
                }
                Err(error) => return Err(error),
            }
            catalog.transforms.insert(transform, !refusals.contains_key(&transform));
        }
        let plan = self.auto.plan(&catalog, grid, self.default_srid);
        for decision in &plan.decisions {
            log_decision(decision, grid, &refusals);
        }

        self.merge_configured_tables(&catalog.tables, &all_schemas, &mut specs, &mut warnings);
        for (id, info) in plan.tables {
            let id = self.resolve_id(&id, &info);
            specs.insert(id, SourceSpec::Table(info));
        }
        self.merge_configured_functions(&catalog.functions, &all_schemas, &mut specs, &mut warnings);
        for (id, info, sql) in plan.functions {
            let id = self.resolve_id(&id, &info);
            specs.insert(id, SourceSpec::Function(info, sql));
        }

        self.pool.check_postgis().await?;
        Ok((specs, warnings))
    }

    /// Merges each configured table with its catalog entry, inserting a [`SourceSpec::Table`] per id.
    fn merge_configured_tables(
        &self,
        db_tables_info: &SqlTableInfoMapMapMap,
        all_schemas: &BTreeSet<String>,
        specs: &mut BTreeMap<String, SourceSpec>,
        warnings: &mut Vec<TileSourceWarning>,
    ) {
        // The same table on two tile grids is two sources on purpose, so only a repeat on one grid is a duplicate.
        let mut declared = HashSet::<(&str, &str, &str, Option<&str>, Option<&str>)>::new();
        for (id, cfg_inf) in &self.tables {
            match self.build_one_table_info(db_tables_info, all_schemas, id, cfg_inf) {
                Ok(merged_inf) => {
                    if !declared.insert((
                        &cfg_inf.schema,
                        &cfg_inf.table,
                        &cfg_inf.geometry_column,
                        cfg_inf.filter.as_deref(),
                        cfg_inf.tile_grid.as_deref(),
                    )) {
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
    }

    /// Merges each configured function with its catalog entry, inserting a [`SourceSpec::Function`] per id.
    /// A function's SQL is already known at catalog time, so the spec carries it directly.
    fn merge_configured_functions(
        &self,
        db_funcs_info: &SqlFuncInfoMapMap,
        all_schemas: &BTreeSet<String>,
        specs: &mut BTreeMap<String, SourceSpec>,
        warnings: &mut Vec<TileSourceWarning>,
    ) {
        // The same function on two tile grids is two sources on purpose, so only a repeat on one grid is a duplicate.
        let mut used_on_grid = HashSet::<(&str, &str, Option<&str>)>::new();
        for (id, cfg_inf) in &self.functions {
            match Self::build_one_function_info(db_funcs_info, all_schemas, id, cfg_inf) {
                Ok((merged_inf, pg_sql_info)) => {
                    if !used_on_grid.insert((&cfg_inf.schema, &cfg_inf.function, cfg_inf.tile_grid.as_deref())) {
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
    }

    /// Turns one [`SourceSpec`] into a running [`PostgresSource`].
    ///
    /// The slow half of discovery: a table builds its SQL and computes its bounds here (the work [`discover`](Self::discover) deferred); a function is cheap, as its SQL is already known.
    /// The returned [`SourceSpec`] carries any computed bounds back for `--save-config`.
    pub async fn instantiate(&self, id: &str, spec: SourceSpec) -> PostgresResult<(BoxedSource, SourceSpec)> {
        match spec {
            SourceSpec::Table(info) => {
                let grid = self.tile_grid_for(info.tile_grid.as_deref());
                if grid.contains_pole() && info.srid != grid.srid() {
                    warn!(
                        source.id = id,
                        table.srid = info.srid,
                        tile_grid = grid.grid().id(),
                        tile_grid.crs = %grid.grid().crs(),
                        "A pole lies inside the tile grid and the table is stored in another CRS, so tiles around the pole may miss features. Storing the data in the tile grid's CRS avoids that."
                    );
                }
                let (id, tiler, info) = table_to_query(
                    id.to_owned(),
                    info,
                    self.pool.clone(),
                    self.auto_bounds,
                    self.max_feature_count,
                    self.order_by,
                    grid,
                )
                .await?;
                match &tiler {
                    PostgresTiler::Sql(pg_sql) => {
                        trace!(source.id = %id, sql = %pg_sql.sql_query, "source SQL query");
                    }
                    #[cfg(feature = "unstable-generate")]
                    PostgresTiler::Engine(engine) => {
                        trace!(source.id = %id, sql = ?engine.sql_queries, "source feature queries");
                    }
                }
                let cache = info.cache.clone().unwrap_or_default();
                let source = self.build_source(id, &info, tiler, &cache, grid);
                Ok((source, SourceSpec::Table(info)))
            }
            SourceSpec::Function(info, pg_sql) => {
                trace!(source.id = %id, sql = %pg_sql.sql_query, "source SQL query");
                let grid = self.tile_grid_for(info.tile_grid.as_deref());
                let cache = info.cache.clone().unwrap_or_default();
                let source = self.build_source(id.to_owned(), &info, pg_sql.clone(), &cache, grid);
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
        all_schemas: &BTreeSet<String>,
        id: &String,
        table_info_from_config: &TableInfo,
    ) -> Result<TableInfo, String> {
        let table_infos_for_schema = find_schema_info(
            table_infos_from_db,
            all_schemas,
            &table_info_from_config.schema,
            "tables with a geometry column",
            id,
        )?;
        let table_infos_for_table = find_info(table_infos_for_schema, &table_info_from_config.table, "table", id)?;
        let table_info_for_geometry_column = find_info(
            table_infos_for_table,
            &table_info_from_config.geometry_column,
            "geometry column",
            id,
        )?;
        let default_srid = self.default_srid_for(table_info_from_config.tile_grid.as_deref());
        let merged_table_info = table_info_for_geometry_column
            .append_cfg_info(table_info_from_config, id, default_srid)
            .ok_or_else(|| format!("Failed to merge config info for table {id}"))?;
        self.check_srid_fits_tile_grid(&merged_table_info)?;
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
        function_infos_from_db: &BTreeMap<String, BTreeMap<String, (PostgresSqlInfo, FunctionInfo)>>,
        all_schemas: &BTreeSet<String>,
        id: &str,
        function_info_from_config: &FunctionInfo,
    ) -> Result<(FunctionInfo, PostgresSqlInfo), String> {
        let function_infos_for_schema = find_schema_info(
            function_infos_from_db,
            all_schemas,
            &function_info_from_config.schema,
            "tile-serving functions (z,x,y) -> bytea",
            id,
        )?;
        let function_name = &function_info_from_config.function;
        let (function_sql_info, table_info_from_schema) =
            find_info(function_infos_for_schema, function_name, "function", id)?;
        let merged_function_info = table_info_from_schema.append_cfg_info(function_info_from_config);
        Ok((merged_function_info, function_sql_info.clone()))
    }

    fn resolve_id<T: PostgresInfo>(&self, id: &str, src_inf: &T) -> String {
        let signature = format!("{}.{}", self.pool.get_id(), src_inf.format_id());
        self.id_resolver.resolve(id, signature)
    }

    /// The grid a source is served in, the one it names or else the connection's default.
    fn tile_grid_for(&self, name: Option<&str>) -> &PgTileGrid {
        let name = name.unwrap_or(&self.default_tile_grid);
        self.tile_grids
            .get(name)
            .expect("every grid a source of this connection can name was resolved when the builder was created")
    }

    /// The SRID a table without one is read in, which on a simple grid is the plain planar SRID 0.
    fn default_srid_for(&self, tile_grid: Option<&str>) -> Option<i32> {
        if self.tile_grid_for(tile_grid).grid().is_simple() {
            Some(0)
        } else {
            self.default_srid
        }
    }

    /// Refuses a table with an SRID on a simple grid, which has no CRS to convert it to.
    fn check_srid_fits_tile_grid(&self, info: &TableInfo) -> Result<(), String> {
        let grid = self.tile_grid_for(info.tile_grid.as_deref()).grid();
        if grid.is_simple() && info.srid != 0 {
            return Err(format!(
                "Table {} has SRID={}, but only SRID 0 can be served on the simple tile grid {}",
                info.format_id(),
                info.srid,
                grid.id()
            ));
        }
        Ok(())
    }

    /// Constructs a [`PostgresSource`] from a resolved source description and its SQL.
    /// The given `cache` falls back to the builder's default policy.
    fn build_source(
        &self,
        id: String,
        pg_info: &impl PostgresInfo,
        tiler: impl Into<PostgresTiler>,
        cache: &CachePolicy,
        grid: &PgTileGrid,
    ) -> BoxedSource {
        let mut tilejson = pg_info.to_tilejson(id.clone());
        if !grid.is_web_mercator() {
            // TileJSON has no field for this, so the grid travels as a vendor key with the same shape
            // MapLibre GL JS takes for a custom projection
            tilejson.other.insert(
                "tileGrid".to_owned(),
                serde_json::to_value(grid.grid()).expect("a validated tile grid holds only finite numbers and strings"),
            );
        }
        let tile_info = pg_info.tile_info();
        let cache = cache.or(&self.default_cache);
        BackendSource::Postgres(PostgresSource::new(
            id,
            tiler,
            tilejson,
            self.pool.clone(),
            tile_info,
            cache.zoom(),
            grid.grid().clone(),
        ))
        .boxed()
    }
}

/// Logs one auto-publish decision at the level it deserves.
fn log_decision(decision: &Decision, grid: &PgTileGrid, refusals: &HashMap<Transform, String>) {
    match decision {
        Decision::ScannedTables { schemas, id_format } => info!(
            schemas = %schemas.join(", "),
            source_id_format = %id_format,
            "Auto-publishing tables"
        ),
        Decision::ScannedFunctions { schemas, id_format } => info!(
            schemas = %schemas.join(", "),
            source_id_format = %id_format,
            "Auto-publishing functions"
        ),
        Decision::Skipped {
            relation,
            reason: SkipReason::NoSrid,
        } => warn!(
            table.id = %relation,
            "Table has SRID=0, skipping. To use this table source, set default or specify this table SRID in the config file, or set the default SRID with  --default-srid=..."
        ),
        Decision::Skipped {
            relation,
            reason: SkipReason::SridOnSimpleGrid(srid),
        } => warn!(
            error = %format!(
                "Table {relation} has SRID={srid}, but only SRID 0 can be served on the simple tile grid {}",
                grid.grid().id()
            ),
            "Skipping table"
        ),
        Decision::Skipped {
            relation,
            reason: SkipReason::CannotTransform(transform),
        } => warn!(
            schema = %relation.schema,
            table = %relation.table,
            geometry_column = %relation.geometry_column,
            table.srid = transform.table,
            tile_grid = %grid.grid().id(),
            tile_grid.crs = %grid.grid().crs(),
            error = %refusals.get(transform).map_or("", String::as_str),
            "PostGIS cannot convert the table to the CRS of its tile grid, skipping"
        ),
        Decision::DefaultSrid { relation, srid } => info!(
            table.id = %relation,
            srid.default = srid,
            "Table has SRID=0, using provided default SRID"
        ),
        Decision::IdColumn { relation, outcome } => match outcome {
            IdColumnOutcome::CaseInsensitive { requested, found } => info!(
                table.id = %relation,
                id_column.requested = %requested,
                id_column.found = %found,
                "id_column not found by exact name, using case-insensitive match"
            ),
            IdColumnOutcome::Ambiguous { requested, candidates } => error!(
                table.id = %relation,
                id_column.requested = %requested,
                id_column.candidates = %candidates.join(", "),
                "Unable to configure source: id_column has no exact match or more than one potential match"
            ),
            IdColumnOutcome::NotInteger { column, typ } => warn!(
                schema = %relation.schema,
                table = %relation.table,
                column = %column,
                column.type = %typ,
                "Unable to use column as a tile feature ID because it has a non-integer type"
            ),
            IdColumnOutcome::NoneFound { searched } => info!(
                schema = %relation.schema,
                table = %relation.table,
                searched = %searched.join(", "),
                "No ID column found for table - searched for an integer column"
            ),
        },
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

#[cfg(all(test, feature = "test-pg"))]
mod tests {
    use std::assert_matches;

    use indoc::indoc;
    use insta::{assert_debug_snapshot, assert_yaml_snapshot};

    use super::*;
    use crate::test_support::pg::{builder_for, seed};

    #[tokio::test]
    async fn discover_and_instantiate_table() {
        let (builder, _container, connstr) = builder_for(indoc! {r"
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
            &connstr,
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
    async fn configured_table_without_geometry_type_takes_the_column_type() {
        let (builder, _container, connstr) = builder_for(indoc! {r"
            tables:
              my_roads:
                schema: public
                table: roads
                geometry_column: geom
                srid: 4326
        "})
        .await;
        seed(
            &connstr,
            "CREATE TABLE public.roads (gid serial PRIMARY KEY, geom geometry(LineString, 4326));",
        )
        .await;

        let (specs, warnings) = builder.discover().await.expect("discover failed");
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        let SourceSpec::Table(info) = specs.get("my_roads").expect("spec for my_roads") else {
            panic!("expected a Table spec");
        };
        assert_yaml_snapshot!(info, @"
        schema: public
        table: roads
        srid: 4326
        geometry_column: geom
        geometry_type: LINESTRING
        ");
    }

    #[tokio::test]
    async fn discover_auto_publishes_from_catalog_and_is_rerunnable() {
        let (builder, _container, connstr) = builder_for("{}").await;
        seed(
            &connstr,
            "CREATE TABLE public.roads (gid serial PRIMARY KEY, geom geometry(LineString, 4326));",
        )
        .await;
        seed(&connstr, TILE_FUNCTION_SQL).await;

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
        assert_matches!(
            first.get("my_func"),
            Some(SourceSpec::Function(..)),
            "expected an auto-published function spec for my_func"
        );

        // An idle re-discover must return the same ids, so a future Reloader sees "no change".
        let (second, _) = builder.discover().await.expect("second discover failed");
        let first_ids: Vec<&String> = first.keys().collect();
        let second_ids: Vec<&String> = second.keys().collect();
        assert_eq!(first_ids, second_ids, "discover must return stable ids");

        // ...and the same fingerprints, so the "no change" verdict survives content comparison.
        for (id, spec) in &first {
            assert_eq!(
                spec.fingerprint(),
                second[id].fingerprint(),
                "fingerprint for {id} changed across an idle re-discover"
            );
        }
    }

    const TILE_FUNCTION_SQL: &str = "CREATE FUNCTION public.my_func(z integer, x integer, y integer) \
         RETURNS bytea AS $$ SELECT NULL::bytea $$ LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE;";

    #[tokio::test]
    async fn discover_and_instantiate_function() {
        let (builder, _container, connstr) = builder_for(indoc! {r"
            functions:
              my_func:
                schema: public
                function: my_func
        "})
        .await;
        seed(&connstr, TILE_FUNCTION_SQL).await;

        let (mut specs, warnings) = builder.discover().await.expect("discover failed");
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");

        let SourceSpec::Function(info, sql) = specs.get("my_func").expect("spec for my_func") else {
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
            empty_tile_implies_empty_children: false,
            signature: "public.my_func(integer, integer, integer) -> bytea",
            has_etag_column: false,
            queryless: None,
            row_query: None,
        }
        "#);

        let spec = specs.remove("my_func").expect("spec for my_func");
        let (source, _) = builder.instantiate("my_func", spec).await.expect("instantiate failed");
        assert_eq!(source.get_id(), "my_func");
    }

    #[tokio::test]
    async fn instantiate_failure_surfaces_as_error() {
        let (builder, _container, connstr) = builder_for(indoc! {r"
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
            &connstr,
            "CREATE TABLE public.points (gid serial PRIMARY KEY, geom geometry(Point, 4326));",
        )
        .await;

        let (mut specs, _) = builder.discover().await.expect("discover failed");
        let spec = specs.remove("my_points").expect("spec for my_points");

        // The table vanishes between discover and instantiate.
        seed(&connstr, "DROP TABLE public.points;").await;

        let result = builder.instantiate("my_points", spec).await;
        assert!(
            result.is_err(),
            "instantiating a vanished table must surface an error, not be silently dropped"
        );
    }

    #[tokio::test]
    async fn discover_missing_sources_surface_as_warnings() {
        let (builder, _container, _connstr) = builder_for(indoc! {r"
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
        assert_eq!(warned_ids, HashSet::from(["nonexistent_table", "nonexistent_function"]));
    }

    #[tokio::test]
    async fn schema_errors_distinguish_missing_from_content_less_schemas() {
        let (builder, _container, connstr) = builder_for(indoc! {r"
            tables:
              t_missing_schema:
                schema: ghost_schema
                table: whatever
                geometry_column: geom
                srid: 4326
                geometry_type: POINT
              t_schema_without_geometry:
                schema: plain_schema
                table: plain_table
                geometry_column: geom
                srid: 4326
                geometry_type: POINT

            functions:
              f_missing_schema:
                schema: ghost_schema
                function: whatever
              f_schema_without_functions:
                schema: empty_schema
                function: whatever
        "})
        .await;
        seed(
            &connstr,
            "CREATE SCHEMA empty_schema;
             CREATE SCHEMA plain_schema;
             CREATE TABLE plain_schema.plain_table (id int);",
        )
        .await;

        let (specs, warnings) = builder.discover().await.expect("discover failed");

        assert!(specs.is_empty(), "unexpected specs: {specs:?}");
        assert_debug_snapshot!(warnings, @r#"
        [
            SourceError {
                source_id: "t_missing_schema",
                error: "Unable to configure source t_missing_schema because schema 'ghost_schema' does not exist. Available schemas: empty_schema, plain_schema, public, tiger, tiger_data, topology",
            },
            SourceError {
                source_id: "t_schema_without_geometry",
                error: "Unable to configure source t_schema_without_geometry because schema 'plain_schema' exists but has no tables with a geometry column.",
            },
            SourceError {
                source_id: "f_missing_schema",
                error: "Unable to configure source f_missing_schema because schema 'ghost_schema' does not exist. Available schemas: empty_schema, plain_schema, public, tiger, tiger_data, topology",
            },
            SourceError {
                source_id: "f_schema_without_functions",
                error: "Unable to configure source f_schema_without_functions because schema 'empty_schema' exists but has no tile-serving functions (z,x,y) -> bytea.",
            },
        ]
        "#);
    }

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct DiscoverFixture {
        sql: String,
        config: String,
    }

    #[derive(serde::Serialize)]
    struct Discovered {
        sources: BTreeMap<String, String>,
        warnings: Vec<String>,
    }

    #[test]
    fn discover_matches_each_fixture() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        insta::glob!(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/pg_discover"),
            "*.yaml",
            |path| {
                let text = std::fs::read_to_string(path).expect("read fixture");
                let fixture: DiscoverFixture = serde_saphyr::from_str(&text).expect("parse fixture");
                let (specs, warnings) = runtime.block_on(async {
                    let (builder, _container, connstr) = builder_for(&fixture.config).await;
                    seed(&connstr, &fixture.sql).await;
                    builder.discover().await.expect("discover failed")
                });
                let discovered = Discovered {
                    sources: specs
                        .into_iter()
                        .map(|(id, spec)| {
                            let source = match spec {
                                SourceSpec::Table(info) => format!("table {} SRID {}", info.format_id(), info.srid),
                                SourceSpec::Function(info, _) => format!("function {}", info.format_id()),
                            };
                            (id, source)
                        })
                        .collect(),
                    warnings: warnings.iter().map(ToString::to_string).collect(),
                };
                assert_yaml_snapshot!(discovered);
            }
        );
    }
}
