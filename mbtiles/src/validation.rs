use std::collections::HashSet;
use std::str::from_utf8;

use enum_display::EnumDisplay;
use martin_tile_utils::{Encoding, Format, MAX_ZOOM, TileInfo};
use serde::Serialize;
use serde_json::Value;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row as _, SqliteConnection, SqliteExecutor, query};
use tilejson::TileJSON;
use tracing::{debug, info, warn};

use crate::MbtError::{
    AggHashMismatch, AggHashValueNotFound, FailedIntegrityCheck, IncorrectTileHash,
    InvalidTileIndex,
};
use crate::errors::{MbtError, MbtResult};
use crate::mbtiles::PatchFileInfo;
use crate::queries::{
    has_tiles_with_hash, is_dedup_id_normalized_tables_type, is_flat_tables_type,
    is_flat_with_hash_tables_type, is_normalized_tables_type,
};
use crate::{Mbtiles, get_patch_type, invert_y_value};

/// Metadata key for the aggregate tiles hash value
pub const AGG_TILES_HASH: &str = "agg_tiles_hash";

/// Metadata key for a diff file, describing the eventual [`AGG_TILES_HASH`] value of the resulting tileset once the diff is applied
pub const AGG_TILES_HASH_AFTER_APPLY: &str = "agg_tiles_hash_after_apply";

/// Metadata key for a diff file, describing the expected [`AGG_TILES_HASH`] value of the tileset to which the diff will be applied.
pub const AGG_TILES_HASH_BEFORE_APPLY: &str = "agg_tiles_hash_before_apply";

/// Describes the naming convention used by a normalized `MBTiles` schema.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize)]
pub enum NormalizedSchema {
    /// Standard: `map` + `images` tables, `tile_id` TEXT (md5 hash of `tile_data`)
    Hash,
    /// Alternative: `tiles_shallow` + `tiles_data` tables, `tile_data_id` INTEGER
    DedupId,
}

impl NormalizedSchema {
    /// Name of the table storing tile coordinates (the "map" table).
    #[must_use]
    pub fn map_table(self) -> &'static str {
        match self {
            Self::Hash => "map",
            Self::DedupId => "tiles_shallow",
        }
    }

    /// Name of the table storing tile blobs (the "images" table).
    #[must_use]
    pub fn content_table(self) -> &'static str {
        match self {
            Self::Hash => "images",
            Self::DedupId => "tiles_data",
        }
    }

    /// Returns `true` if the tile id column is an integer (`DedupId` schema).
    #[must_use]
    pub fn uses_integer_tile_id(self) -> bool {
        matches!(self, Self::DedupId)
    }

    /// Name of the foreign key column linking the map table to the images table.
    #[must_use]
    pub fn tile_id_column(self) -> &'static str {
        match self {
            Self::Hash => "tile_id",
            Self::DedupId => "tile_data_id",
        }
    }

    /// Build a `SELECT zoom_level, tile_column, tile_row, tile_data, <id> AS <alias>`
    /// subquery joining the map and images tables for the given database prefix.
    /// Use `join_type` to control `JOIN` vs `LEFT JOIN`.
    #[must_use]
    pub(crate) fn select_tiles_sql(self, db_prefix: &str, alias: &str, join_type: &str) -> String {
        let map = self.map_table();
        let data_table = self.content_table();
        let id = self.tile_id_column();
        format!(
            "SELECT zoom_level, tile_column, tile_row, tile_data, {map}.{id} AS {alias} \
             FROM {db_prefix}.{map} {join_type} {db_prefix}.{data_table} ON {map}.{id} = {data_table}.{id}"
        )
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, EnumDisplay, Serialize)]
#[enum_display(case = "Kebab")]
pub enum MbtType {
    /// Flat `MBTiles` file without any hash values
    ///
    /// The closest to the original `MBTiles` specification.
    /// It stores all tiles in a single table.
    /// This schema is the most efficient when the tileset contains no duplicate tiles.
    ///
    /// See <https://maplibre.org/martin/mbtiles-schema.html#flat> for the concrete schema.
    Flat,
    /// [`MbtType::Flat`] `MBTiles` file with hash values
    ///
    /// Similar to the [`MbtType::Flat`] schema, but also includes a `tile_hash` column that contains a hash value of the `tile_data` column.
    /// Use this schema when the tileset has no duplicate tiles, but you still want to be able to validate the content of each tile individually.
    ///
    /// See <https://maplibre.org/martin/mbtiles-schema.html#flat-with-hash> for the concrete schema.
    FlatWithHash,
    /// Normalized `MBTiles` file
    ///
    /// The most efficient when the tileset contains duplicate tiles.
    /// It stores all tile blobs in a separate table, and stores the tile Z,X,Y coordinates in a mapping table.
    /// The mapping table contains a foreign key column linking to the tile data table.
    ///
    /// The `hash_view` argument specifies whether to create/assume a `tiles_with_hash` view exists.
    /// The `schema` argument describes the naming convention (standard `map`/`images` or alternative `tiles_shallow`/`tiles_data`).
    ///
    /// See <https://maplibre.org/martin/mbtiles-schema.html#normalized> for the concrete schema.
    Normalized {
        hash_view: bool,
        schema: NormalizedSchema,
    },
}

impl MbtType {
    #[must_use]
    pub fn is_normalized(self) -> bool {
        matches!(self, Self::Normalized { .. })
    }

    #[must_use]
    pub fn is_normalized_with_view(self) -> bool {
        matches!(
            self,
            Self::Normalized {
                hash_view: true,
                ..
            }
        )
    }

    /// Returns the [`NormalizedSchema`] if this is a normalized type, `None` otherwise.
    #[must_use]
    pub fn normalized_schema(self) -> Option<NormalizedSchema> {
        match self {
            Self::Normalized { schema, .. } => Some(schema),
            _ => None,
        }
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, EnumDisplay)]
#[enum_display(case = "Kebab")]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum IntegrityCheckType {
    #[default]
    Quick,
    Full,
    Off,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, EnumDisplay)]
#[enum_display(case = "Kebab")]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum AggHashType {
    /// Verify that the aggregate tiles hash value in the metadata table matches the computed value. Used by default.
    #[default]
    Verify,
    /// Update the aggregate tiles hash value in the metadata table
    Update,
    /// Do not check the aggregate tiles hash value
    Off,
}

impl Mbtiles {
    /// Open the mbtiles file and validate its integrity.
    #[hotpath::measure]
    pub async fn open_and_validate(
        &self,
        check_type: IntegrityCheckType,
        agg_hash: AggHashType,
    ) -> MbtResult<String> {
        let mut conn = if agg_hash == AggHashType::Update {
            self.open().await?
        } else {
            self.open_readonly().await?
        };
        self.validate(&mut conn, check_type, agg_hash).await
    }

    /// Validate the integrity of the mbtiles file by:
    /// - sqlite internal integrity check
    /// - tiles' table has the expected column, row, zoom, and data values
    /// - each tile has the correct hash stored
    ///
    /// Depending on the `agg_hash` parameter, the function will either verify or update the aggregate tiles hash value.
    #[hotpath::measure]
    pub async fn validate<T>(
        &self,
        conn: &mut T,
        check_type: IntegrityCheckType,
        agg_hash: AggHashType,
    ) -> MbtResult<String>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        self.check_integrity(&mut *conn, check_type).await?;
        self.check_tiles_type_validity(&mut *conn).await?;
        self.check_each_tile_hash(&mut *conn).await?;
        match agg_hash {
            AggHashType::Verify => self.check_agg_tiles_hashes(conn).await,
            AggHashType::Update => self.update_agg_tiles_hash(conn).await,
            AggHashType::Off => Ok(String::new()),
        }
    }

    /// Get the aggregate tiles hash value from the metadata table
    pub async fn get_agg_tiles_hash<T>(&self, conn: &mut T) -> MbtResult<Option<String>>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        self.get_metadata_value(&mut *conn, AGG_TILES_HASH).await
    }

    /// Detect tile format and verify that it is consistent across some tiles
    #[hotpath::measure]
    pub async fn detect_format<T>(
        &self,
        tilejson: &TileJSON,
        conn: &mut T,
    ) -> MbtResult<Option<TileInfo>>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let mut tile_info = None;
        let mut tested_zoom = -1_i64;
        let mut tiles_detected = false;

        // First, pick any random tile
        let query = query!(
            "SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles WHERE zoom_level >= 0 LIMIT 1"
        );
        let row = query.fetch_optional(&mut *conn).await?;
        if let Some(r) = row {
            tile_info = self.parse_tile(r.zoom_level, r.tile_column, r.tile_row, r.tile_data);
            tested_zoom = r.zoom_level.unwrap_or(-1);
            tiles_detected = tile_info.is_some();
        }

        // Afterward, iterate over tiles in all allowed zooms and check for consistency
        for z in tilejson.minzoom.unwrap_or(0)..=tilejson.maxzoom.unwrap_or(18) {
            if i64::from(z) == tested_zoom {
                continue;
            }
            let query = query! {"SELECT tile_column, tile_row, tile_data FROM tiles WHERE zoom_level = ? LIMIT 1", z};
            let row = query.fetch_optional(&mut *conn).await?;
            if let Some(r) = row {
                match (
                    tile_info,
                    self.parse_tile(Some(z.into()), r.tile_column, r.tile_row, r.tile_data),
                ) {
                    (_, None) => {}
                    (None, new) => {
                        tile_info = new;
                        tiles_detected = true;
                    }
                    (Some(old), Some(new)) if old == new => {}
                    (Some(old), Some(new)) => {
                        return Err(MbtError::InconsistentMetadata(old, new));
                    }
                }
            }
        }

        tile_info = self.check_format_metadata(tilejson, tile_info);
        tile_info = self.check_compression_metadata(tilejson, tile_info, tiles_detected);

        if let Some(info) = tile_info {
            if info.format != Format::Mvt
                && info.format != Format::Mlt
                && tilejson.vector_layers.is_some()
            {
                warn!(
                    "{} has vector_layers metadata value, but the tiles are not MVT/MLT",
                    self.filename()
                );
            }
            Ok(Some(info))
        } else {
            Ok(None)
        }
    }

    fn check_format_metadata(
        &self,
        tilejson: &TileJSON,
        mut tile_info: Option<TileInfo>,
    ) -> Option<TileInfo> {
        if let Some(Value::String(fmt)) = tilejson.other.get("format") {
            let file = self.filename();
            match (tile_info, Format::parse(fmt)) {
                (_, None) => {
                    warn!("Unknown format value in metadata: {fmt}");
                }
                (None, Some(fmt)) => {
                    if fmt.is_detectable() {
                        warn!(
                            mbtiles.file = %file,
                            metadata.format = %fmt,
                            "Metadata table sets detectable tile format, but it could not be verified"
                        );
                    } else {
                        info!(
                            mbtiles.file = %file,
                            metadata.format = %fmt,
                            "Using tile format from metadata table"
                        );
                    }
                    tile_info = Some(fmt.into());
                }
                (Some(info), Some(fmt)) if info.format == fmt => {
                    debug!(
                        mbtiles.file = %file,
                        tile.info = %info,
                        metadata.format = %fmt,
                        "Detected tile format matches metadata.format"
                    );
                }
                (Some(info), _) => {
                    warn!(
                        mbtiles.file = %file,
                        metadata.format = %fmt,
                        tile.info = ?info,
                        "Found inconsistency between metadata.format and detected tile format; tiles will be returned as detected"
                    );
                }
            }
        }
        tile_info
    }

    fn check_compression_metadata(
        &self,
        tilejson: &TileJSON,
        mut tile_info: Option<TileInfo>,
        tiles_detected: bool,
    ) -> Option<TileInfo> {
        if let Some(Value::String(cmp)) = tilejson.other.get("compression") {
            let file = self.filename();
            match Encoding::parse(cmp) {
                None => {
                    warn!("Unknown compression value in metadata: {cmp} in file {file}");
                }
                Some(enc) => match tile_info {
                    None => {
                        info!(
                            mbtiles.file = %file,
                            metadata.compression = %cmp,
                            "Metadata table sets tile compression, but it could not be verified"
                        );
                    }
                    Some(info) if tiles_detected => {
                        // `Uncompressed` and `Internal` both mean "no external compression
                        // algorithm", so treat them as equivalent when validating the metadata.
                        // `Internal` means the format compresses data natively (PNG/JPEG/WebP);
                        // `Uncompressed` is the metadata spelling of "no external encoding".
                        if enc == info.encoding
                            || (!enc.is_encoded() && !info.encoding.is_encoded())
                        {
                            debug!(
                                mbtiles.file = %file,
                                tile.encoding = ?info.encoding,
                                metadata.compression = %cmp,
                                "Detected tile encoding matches metadata.compression"
                            );
                        } else {
                            warn!(
                                mbtiles.file = %file,
                                metadata.compression = %cmp,
                                tile.info = ?info,
                                "Found inconsistency between metadata.compression and detected tile encoding; tiles will be returned as detected"
                            );
                        }
                    }
                    Some(info) => {
                        info!(
                            mbtiles.file = %file,
                            metadata.compression = %cmp,
                            "Using tile compression from metadata table"
                        );
                        tile_info = Some(info.encoding(enc));
                    }
                },
            }
        }
        tile_info
    }

    /// Detects the format of a tile and returns its information if none of the values are `None`
    fn parse_tile(
        &self,
        z: Option<i64>,
        x: Option<i64>,
        y: Option<i64>,
        tile: Option<Vec<u8>>,
    ) -> Option<TileInfo> {
        if let (Some(z), Some(x), Some(y), Some(tile)) = (z, x, y, tile) {
            let info = TileInfo::detect(&tile);
            debug!(
                "Tile {z}/{x}/{} is detected as {info} in file {}",
                {
                    if let (Ok(z), Ok(y)) = (u8::try_from(z), u32::try_from(y)) {
                        invert_y_value(z, y).to_string()
                    } else {
                        format!("{y} (invalid values, cannot invert Y)")
                    }
                },
                self.filename(),
            );
            Some(info)
        } else {
            None
        }
    }

    /// Detect the type of the `MBTiles` file.
    ///
    /// See [`MbtType`] for more information.
    #[hotpath::measure]
    pub async fn detect_type<T>(&self, conn: &mut T) -> MbtResult<MbtType>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        debug!("Detecting MBTiles type for {self}");
        let typ = if is_normalized_tables_type(&mut *conn).await? {
            MbtType::Normalized {
                hash_view: has_tiles_with_hash(&mut *conn).await?,
                schema: NormalizedSchema::Hash,
            }
        } else if is_dedup_id_normalized_tables_type(&mut *conn).await? {
            MbtType::Normalized {
                hash_view: false,
                schema: NormalizedSchema::DedupId,
            }
        } else if is_flat_with_hash_tables_type(&mut *conn).await? {
            MbtType::FlatWithHash
        } else if is_flat_tables_type(&mut *conn).await? {
            MbtType::Flat
        } else {
            return Err(MbtError::InvalidDataFormat(self.filepath().to_string()));
        };

        self.check_for_uniqueness_constraint(&mut *conn, typ)
            .await?;

        Ok(typ)
    }

    async fn check_for_uniqueness_constraint<T>(
        &self,
        conn: &mut T,
        mbt_type: MbtType,
    ) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let table_name = match mbt_type {
            MbtType::Flat => "tiles",
            MbtType::FlatWithHash => "tiles_with_hash",
            MbtType::Normalized { schema, .. } => schema.map_table(),
        };

        let indexes = query("SELECT name FROM pragma_index_list(?) WHERE [unique] = 1")
            .bind(table_name)
            .fetch_all(&mut *conn)
            .await?;

        // Ensure there is some index on tiles that has a unique constraint on (zoom_level, tile_row, tile_column)
        for index in indexes {
            let mut unique_idx_cols = HashSet::new();
            let rows = query("SELECT DISTINCT name FROM pragma_index_info(?)")
                .bind(index.get::<String, _>("name"))
                .fetch_all(&mut *conn)
                .await?;

            for row in rows {
                unique_idx_cols.insert(row.get("name"));
            }

            if unique_idx_cols
                .symmetric_difference(&HashSet::from([
                    "zoom_level".to_string(),
                    "tile_column".to_string(),
                    "tile_row".to_string(),
                ]))
                .collect::<Vec<_>>()
                .is_empty()
            {
                return Ok(());
            }
        }

        Err(MbtError::NoUniquenessConstraint(
            self.filepath().to_string(),
        ))
    }

    /// Perform `SQLite` internal integrity check
    #[hotpath::measure]
    pub async fn check_integrity<T>(
        &self,
        conn: &mut T,
        integrity_check: IntegrityCheckType,
    ) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        if integrity_check == IntegrityCheckType::Off {
            info!(mbtiles.file = %self, "Skipping integrity check");
            return Ok(());
        }

        let sql = if integrity_check == IntegrityCheckType::Full {
            "PRAGMA integrity_check;"
        } else {
            "PRAGMA quick_check;"
        };

        let result: Vec<String> = query(sql)
            .map(|row: SqliteRow| row.get(0))
            .fetch_all(&mut *conn)
            .await?;

        if result.len() > 1
            || result.first().ok_or(FailedIntegrityCheck(
                self.filepath().to_string(),
                vec!["SQLite could not perform integrity check".to_string()],
            ))? != "ok"
        {
            return Err(FailedIntegrityCheck(self.filepath().to_string(), result));
        }

        info!(
            mbtiles.file = %self,
            integrity_check = ?integrity_check,
            "Integrity check passed"
        );
        Ok(())
    }

    /// Check that the tiles table has the expected column, row, zoom, and data values
    #[hotpath::measure]
    pub async fn check_tiles_type_validity<T>(&self, conn: &mut T) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let sql = format!(
            "
SELECT zoom_level, tile_column, tile_row
FROM tiles
WHERE FALSE
   OR typeof(zoom_level) != 'integer'
   OR zoom_level < 0
   OR zoom_level > {MAX_ZOOM}
   OR typeof(tile_column) != 'integer'
   OR tile_column < 0
   OR tile_column >= (1 << zoom_level)
   OR typeof(tile_row) != 'integer'
   OR tile_row < 0
   OR tile_row >= (1 << zoom_level)
   OR (typeof(tile_data) != 'blob' AND typeof(tile_data) != 'null')
LIMIT 1;"
        );

        if let Some(row) = query(&sql).fetch_optional(&mut *conn).await? {
            let mut res: Vec<String> = Vec::with_capacity(3);
            for idx in (0..3).rev() {
                use sqlx::ValueRef as _;
                let raw = row.try_get_raw(idx)?;
                if raw.is_null() {
                    res.push("NULL".to_string());
                } else if let Ok(v) = row.try_get::<String, _>(idx) {
                    res.push(format!(r#""{v}" (TEXT)"#));
                } else if let Ok(v) = row.try_get::<Vec<u8>, _>(idx) {
                    res.push(format!(
                        r#""{}" (BLOB)"#,
                        from_utf8(&v).unwrap_or("<non-utf8-data>")
                    ));
                } else if let Ok(v) = row.try_get::<i32, _>(idx) {
                    res.push(format!("{v}"));
                } else if let Ok(v) = row.try_get::<f64, _>(idx) {
                    res.push(format!("{v} (REAL)"));
                } else {
                    res.push(format!("{:?}", raw.type_info()));
                }
            }

            let [tile_row, tile_column, zoom_level]: [String; 3] =
                res.try_into().expect("res should contain exactly 3 items");
            return Err(InvalidTileIndex(
                self.filepath().to_string(),
                zoom_level,
                tile_column,
                tile_row,
            ));
        }

        info!(mbtiles.file = %self, "All values in the `tiles` table/view are valid");
        Ok(())
    }

    #[hotpath::measure]
    pub async fn check_agg_tiles_hashes<T>(&self, conn: &mut T) -> MbtResult<String>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let Some(stored) = self.get_agg_tiles_hash(&mut *conn).await? else {
            return Err(AggHashValueNotFound(self.filepath().to_string()));
        };
        let computed = calc_agg_tiles_hash(&mut *conn).await?;
        if stored != computed {
            let file = self.filepath().to_string();
            return Err(AggHashMismatch(computed, stored, file));
        }

        info!(
            mbtiles.file = %self,
            agg_tiles_hash = %computed,
            "agg_tiles_hash has been verified"
        );
        Ok(computed)
    }

    /// Compute new aggregate tiles hash and save it to the metadata table (if needed)
    #[hotpath::measure]
    pub async fn update_agg_tiles_hash<T>(&self, conn: &mut T) -> MbtResult<String>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let old_hash = self.get_agg_tiles_hash(&mut *conn).await?;
        let hash = calc_agg_tiles_hash(&mut *conn).await?;
        if old_hash.as_ref() == Some(&hash) {
            info!(
                mbtiles.file = %self,
                agg_tiles_hash = %hash,
                "Metadata value agg_tiles_hash is already set to the correct hash"
            );
        } else {
            if let Some(old_hash) = old_hash {
                info!(
                    mbtiles.file = %self,
                    agg_tiles_hash.old = %old_hash,
                    agg_tiles_hash.new = %hash,
                    "Updating agg_tiles_hash"
                );
            } else {
                info!(
                    mbtiles.file = %self,
                    agg_tiles_hash = %hash,
                    "Adding a new metadata value agg_tiles_hash"
                );
            }
            self.set_metadata_value(&mut *conn, AGG_TILES_HASH, &hash)
                .await?;
        }
        Ok(hash)
    }

    #[hotpath::measure]
    pub async fn check_each_tile_hash<T>(&self, conn: &mut T) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        // Note that hex() always returns upper-case HEX values
        let sql = match self.detect_type(&mut *conn).await? {
            MbtType::Flat => {
                info!(
                    mbtiles.file = %self,
                    "Skipping per-tile hash validation because this is a flat MBTiles file"
                );
                return Ok(());
            }
            MbtType::FlatWithHash => {
                "SELECT expected, computed FROM (
                    SELECT
                        upper(tile_hash) AS expected,
                        md5_hex(tile_data) AS computed
                    FROM tiles_with_hash
                ) AS t
                WHERE expected != computed
                LIMIT 1;"
            }
            MbtType::Normalized { schema, .. } => {
                let map = schema.map_table();
                let data_table = schema.content_table();
                let id = schema.tile_id_column();
                // Check that all tile references in the map table exist in the data table
                let sql = format!(
                    "SELECT CAST(m.{id} AS TEXT)
                     FROM {map} m
                     WHERE m.{id} IS NOT NULL
                       AND NOT EXISTS (
                           SELECT 1
                           FROM {data_table} d
                           WHERE d.{id} = m.{id}
                       )
                     LIMIT 1;"
                );
                if let Some(row) = query(&sql).fetch_optional(&mut *conn).await? {
                    let missing_id: String = row.get(0);
                    return Err(MbtError::MissingTileReference(
                        self.filepath().to_string(),
                        missing_id,
                        data_table,
                    ));
                }

                // For Hash schema, also verify that tile_id == md5_hex(tile_data)
                if matches!(schema, NormalizedSchema::Hash) {
                    let sql = format!(
                        "SELECT expected, computed FROM (
                            SELECT
                                upper(CAST(d.{id} AS TEXT)) AS expected,
                                md5_hex(d.tile_data) AS computed
                            FROM {data_table} d
                        ) AS t
                        WHERE expected != computed
                        LIMIT 1;"
                    );
                    if let Some(row) = query(&sql).fetch_optional(&mut *conn).await? {
                        return Err(IncorrectTileHash(
                            self.filepath().to_string(),
                            row.get(0),
                            row.get(1),
                        ));
                    }
                }

                info!(mbtiles.file = %self, "All tile hashes are valid");
                return Ok(());
            }
        };

        query(sql)
            .fetch_optional(&mut *conn)
            .await?
            .map_or(Ok(()), |v| {
                Err(IncorrectTileHash(
                    self.filepath().to_string(),
                    v.get(0),
                    v.get(1),
                ))
            })?;

        info!(mbtiles.file = %self, "All tile hashes are valid");
        Ok(())
    }

    pub async fn examine_diff(&self, conn: &mut SqliteConnection) -> MbtResult<PatchFileInfo> {
        let info = PatchFileInfo {
            mbt_type: self.detect_type(&mut *conn).await?,
            agg_tiles_hash: self.get_agg_tiles_hash(&mut *conn).await?,
            agg_tiles_hash_before_apply: self
                .get_metadata_value(&mut *conn, AGG_TILES_HASH_BEFORE_APPLY)
                .await?,
            agg_tiles_hash_after_apply: self
                .get_metadata_value(&mut *conn, AGG_TILES_HASH_AFTER_APPLY)
                .await?,
            patch_type: get_patch_type(conn).await?,
        };

        Ok(info)
    }

    pub fn assert_hashes(&self, info: &PatchFileInfo, force: bool) -> MbtResult<()> {
        if info.agg_tiles_hash.is_none() {
            if !force {
                return Err(MbtError::CannotDiffFileWithoutHash(
                    self.filepath().to_string(),
                ));
            }
            warn!(
                "File {self} has no {AGG_TILES_HASH} metadata field, probably because it was created by an older version of the `mbtiles` tool.  Use this command to update the value:\nmbtiles validate --agg-hash update {self}"
            );
        } else if info.agg_tiles_hash_before_apply.is_some()
            || info.agg_tiles_hash_after_apply.is_some()
        {
            if !force {
                return Err(MbtError::DiffingDiffFile(self.filepath().to_string()));
            }
            warn!(
                "File {self} has {AGG_TILES_HASH_BEFORE_APPLY} or {AGG_TILES_HASH_AFTER_APPLY} metadata field, indicating it is a patch file which should not be diffed with another file."
            );
        }
        Ok(())
    }

    pub fn validate_diff_info(&self, info: &PatchFileInfo, force: bool) -> MbtResult<()> {
        match (
            &info.agg_tiles_hash_before_apply,
            &info.agg_tiles_hash_after_apply,
        ) {
            (Some(before), Some(after)) => {
                info!(
                    "The patch file {self} expects to be applied to a tileset with {AGG_TILES_HASH}={before}, and should result in hash {after} after applying",
                );
            }
            (None, Some(_)) => {
                if !force {
                    return Err(MbtError::PatchFileHasNoBeforeHash(
                        self.filepath().to_string(),
                    ));
                }
                warn!(
                    "The patch file {self} has no {AGG_TILES_HASH_BEFORE_APPLY} metadata field, probably because it was created by an older version of the `mbtiles` tool."
                );
            }
            _ => {
                if !force {
                    return Err(MbtError::PatchFileHasNoHashes(self.filepath().to_string()));
                }
                warn!(
                    "The patch file {self} has no {AGG_TILES_HASH_AFTER_APPLY} metadata field, probably because it was not properly created by the `mbtiles` tool."
                );
            }
        }
        Ok(())
    }
}

/// Compute the hash of the combined tiles in the mbtiles file tiles table/view.
/// This should work on all mbtiles files perf `MBTiles` specification.
#[hotpath::measure]
pub async fn calc_agg_tiles_hash<T>(conn: &mut T) -> MbtResult<String>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Calculating agg_tiles_hash");
    let query = query(
        // The md5_concat func will return NULL if there are no rows in the tiles table.
        // For our use case, we will treat it as an empty string, and hash that.
        // `tile_data` values must be stored as a blob per MBTiles spec
        // `md5` functions will fail if the value is not text/blob/null
        //
        // Note that ORDER BY controls the output ordering, which is important for the hash value,
        // and we must use ORDER BY as a parameter to the aggregate function itself (available since SQLite 3.44.0)
        // See https://sqlite.org/forum/forumpost/228bb96e12a746ce
        "
SELECT coalesce(
           md5_concat_hex(
               cast(zoom_level AS text),
               cast(tile_column AS text),
               cast(tile_row AS text),
               tile_data
               ORDER BY zoom_level, tile_column, tile_row),
           md5_hex(''))
FROM tiles;
",
    );
    Ok(query.fetch_one(conn).await?.get::<String, _>(0))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::mbtiles::tests::open;
    use crate::metadata::anonymous_mbtiles;

    #[actix_rt::test]
    async fn detect_type() {
        let script = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
        let (mbt, mut conn) = anonymous_mbtiles(script).await;
        let res = mbt.detect_type(&mut conn).await.unwrap();
        assert_eq!(res, MbtType::Flat);

        let script = include_str!("../../tests/fixtures/mbtiles/zoomed_world_cities.sql");
        let (mbt, mut conn) = anonymous_mbtiles(script).await;
        let res = mbt.detect_type(&mut conn).await.unwrap();
        assert_eq!(res, MbtType::FlatWithHash);

        let script = include_str!("../../tests/fixtures/mbtiles/geography-class-jpg.sql");
        let (mbt, mut conn) = anonymous_mbtiles(script).await;
        let res = mbt.detect_type(&mut conn).await.unwrap();
        assert_eq!(
            res,
            MbtType::Normalized {
                hash_view: false,
                schema: NormalizedSchema::Hash
            }
        );

        let script = include_str!("../../tests/fixtures/mbtiles/normalized-dedup-id.sql");
        let (mbt, mut conn) = anonymous_mbtiles(script).await;
        let res = mbt.detect_type(&mut conn).await.unwrap();
        assert_eq!(
            res,
            MbtType::Normalized {
                hash_view: false,
                schema: NormalizedSchema::DedupId
            }
        );

        let (mut conn, mbt) = open(":memory:").await.unwrap();
        let res = mbt.detect_type(&mut conn).await;
        assert!(matches!(res, Err(MbtError::InvalidDataFormat(_))));
    }

    #[actix_rt::test]
    async fn validate_valid_file() {
        let script = include_str!("../../tests/fixtures/mbtiles/zoomed_world_cities.sql");
        let (mbt, mut conn) = anonymous_mbtiles(script).await;
        mbt.check_integrity(&mut conn, IntegrityCheckType::Quick)
            .await
            .unwrap();
    }

    #[actix_rt::test]
    async fn validate_invalid_file() {
        let script = include_str!("../../tests/fixtures/files/invalid_zoomed_world_cities.sql");
        let (mbt, mut conn) = anonymous_mbtiles(script).await;
        let result = mbt.check_agg_tiles_hashes(&mut conn).await;
        assert!(matches!(result, Err(AggHashMismatch(..))));
    }

    #[actix_rt::test]
    async fn check_tile_hash_valid_normalized_hash() {
        let script = include_str!("../../tests/fixtures/mbtiles/geography-class-png.sql");
        let (mbt, mut conn) = anonymous_mbtiles(script).await;
        // Should pass — tile_id values in images match md5_hex(tile_data)
        mbt.check_each_tile_hash(&mut conn).await.unwrap();
    }

    #[actix_rt::test]
    async fn check_tile_hash_detects_corrupted_normalized_hash() {
        let (mbt, mut conn) = anonymous_mbtiles(
            "CREATE TABLE map (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_id TEXT);
             CREATE TABLE images (tile_data BLOB, tile_id TEXT);
             CREATE TABLE metadata (name TEXT, value TEXT);
             CREATE UNIQUE INDEX map_index ON map (zoom_level, tile_column, tile_row);
             CREATE UNIQUE INDEX images_id ON images (tile_id);
             INSERT INTO metadata VALUES('name','test');
             INSERT INTO images VALUES(X'0102030405', 'wrong_hash_value');
             INSERT INTO map VALUES(0, 0, 0, 'wrong_hash_value');
             CREATE VIEW tiles AS SELECT map.zoom_level, map.tile_column, map.tile_row, images.tile_data FROM map JOIN images ON map.tile_id = images.tile_id;",
        )
        .await;
        let result = mbt.check_each_tile_hash(&mut conn).await;
        assert!(
            matches!(result, Err(IncorrectTileHash(..))),
            "should detect that tile_id != md5_hex(tile_data), got {result:?}"
        );
    }
}
