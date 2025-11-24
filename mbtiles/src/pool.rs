use std::path::Path;

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Pool, Sqlite, SqlitePool};

use crate::errors::MbtResult;
use crate::{MbtType, Mbtiles, Metadata};

/// Connection pool for concurrent read access to an `MBTiles` file.
///
/// `MbtilesPool` wraps an [`Mbtiles`] reference with a `SQLite` connection pool,
/// enabling safe concurrent access for tile serving applications. This is the
/// recommended type for production tile servers.
///
/// # Connection Pooling
///
/// The pool manages multiple `SQLite` connections to the same file, allowing
/// concurrent read operations without blocking. This is particularly useful
/// for web servers handling multiple simultaneous tile requests.
///
/// # Examples
///
/// ## Basic tile serving
///
/// ```
/// use mbtiles::MbtilesPool;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Open file with connection pool
/// let pool = MbtilesPool::open_readonly("world.mbtiles").await?;
///
/// // Get metadata (automatically acquires connection from pool)
/// let metadata = pool.get_metadata().await?;
/// println!("Tileset: {}", metadata.tilejson.name.unwrap_or_default());
///
/// // Fetch a tile - connection is automatically managed
/// if let Some(tile_data) = pool.get_tile(4, 5, 6).await? {
///     println!("Tile size: {} bytes", tile_data.len());
/// }
/// # Ok(())
/// # }
/// ```
///
/// ## Concurrent tile requests
///
/// ```
/// use mbtiles::MbtilesPool;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let pool = MbtilesPool::open_readonly("world.mbtiles").await?;
///
/// // Spawn multiple concurrent tile requests
/// let mut handles = vec![];
/// for z in 0..5 {
///     // cheap and thead-save
///     let pool = pool.clone();
///     handles.push(tokio::spawn(async move {
///         pool.get_tile(z, 0, 0).await
///     }));
/// }
///
/// // All requests run concurrently using different pool connections
/// for handle in handles {
///     let result = handle.await??;
///     println!("Got tile: {:?}", result.is_some());
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct MbtilesPool {
    mbtiles: Mbtiles,
    pool: Pool<Sqlite>,
}

impl MbtilesPool {
    /// Opens an `MBTiles` file in read-only mode with connection pooling.
    ///
    /// Creates a new connection pool for the specified file, enabling safe
    /// concurrent read access. This is the primary way to open an `MBTiles` file
    /// for serving tiles in a production application.
    ///
    /// # Connection Pool
    ///
    /// The pool automatically manages multiple `SQLite` connections, allowing
    /// concurrent tile requests without blocking. Each method call acquires
    /// a connection from the pool, executes the operation, and returns the
    /// connection to the pool automatically.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file does not exist
    /// - The file is not a valid `SQLite` database
    /// - The connection pool cannot be created
    ///
    /// # Examples
    ///
    /// ```
    /// use mbtiles::MbtilesPool;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// // Open for concurrent tile serving
    /// let pool = MbtilesPool::open_readonly("world.mbtiles").await?;
    ///
    /// // Pool automatically manages connections for all operations
    /// let metadata = pool.get_metadata().await?;
    /// let tile = pool.get_tile(4, 5, 6).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn open_readonly<P: AsRef<Path>>(filepath: P) -> MbtResult<Self> {
        let mbtiles = Mbtiles::new(filepath)?;
        let opt = SqliteConnectOptions::new()
            .filename(mbtiles.filepath())
            .read_only(true);
        let pool = SqlitePool::connect_with(opt).await?;
        Ok(Self { mbtiles, pool })
    }

    /// Retrieves the metadata for the `MBTiles` file.
    ///
    /// Returns a [`Metadata`] struct containing:
    /// - `TileJSON` information (name, description, bounds, zoom levels, etc.)
    /// - Tile format and encoding
    /// - Layer type (overlay or baselayer)
    /// - Additional JSON metadata
    /// - Aggregate tiles hash (if available)
    ///
    /// This method automatically acquires a connection from the pool.
    ///
    /// # Examples
    ///
    /// ```
    /// use mbtiles::MbtilesPool;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let pool = MbtilesPool::open_readonly("world.mbtiles").await?;
    /// let metadata = pool.get_metadata().await?;
    ///
    /// println!("Tileset: {}", metadata.tilejson.name.unwrap_or_default());
    /// println!("Format: {}", metadata.tile_info.format);
    /// println!("Zoom levels: {:?}-{:?}", metadata.tilejson.minzoom, metadata.tilejson.maxzoom);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_metadata(&self) -> MbtResult<Metadata> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles.get_metadata(&mut *conn).await
    }

    /// Detects the schema type of the `MBTiles` file.
    ///
    /// Examines the database schema to determine which of the three `MBTiles`
    /// schema types is in use:
    /// - [`MbtType::Flat`] - Single `tiles` table
    /// - [`MbtType::FlatWithHash`] - `tiles_with_hash` table
    /// - [`MbtType::Normalized`] - Separate `map` and `images` tables
    ///
    /// You typically need the schema type for operations like [`get_tile_and_hash`](Self::get_tile_and_hash)
    /// or [`contains`](Self::contains) that behave differently based on the schema.
    ///
    /// > [!TIP]
    /// > This method queries the database schema.
    /// > Consider caching the result if you need it repeatedly.
    ///
    /// # Examples
    ///
    /// ```
    /// use mbtiles::{MbtilesPool, MbtType};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let pool = MbtilesPool::open_readonly("tiles.mbtiles").await?;
    /// let mbt_type = pool.detect_type().await?;
    ///
    /// match mbt_type {
    ///     MbtType::Flat => println!("Simple flat schema"),
    ///     MbtType::FlatWithHash => println!("Flat schema with hashes"),
    ///     MbtType::Normalized { .. } => println!("Normalized schema with deduplication"),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn detect_type(&self) -> MbtResult<MbtType> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles.detect_type(&mut *conn).await
    }

    /// Retrieves a tile from the pool by its coordinates.
    ///
    /// Automatically acquires a connection from the pool, fetches the tile data,
    /// and returns the connection to the pool. Safe to call concurrently from
    /// multiple tasks.
    ///
    /// # Coordinate System
    ///
    /// Coordinates use the [xyz Slippy map tilenames](https://wiki.openstreetmap.org/wiki/Slippy_map_tilenames) tile scheme where:
    /// - `z` is the zoom level (0-30)
    /// - `x` is the column (0 to 2^z - 1)
    /// - `y` is the row in XYZ format (0 at top, increases southward)
    ///
    /// > [!NOTE]
    /// > MBTiles files internally use [osgeos' Tile Map Service](https://wiki.openstreetmap.org/wiki/TMS) coordinates (0 at bottom).
    /// > This method handles the conversion automatically as maplibre/mapbox expect this.
    ///
    /// # Returns
    ///
    /// - `Ok(Some(data))` if the tile exists
    /// - `Ok(None)` if no tile exists at the coordinates
    /// - `Err(_)` on database errors
    ///
    /// # Examples
    ///
    /// ```
    /// use mbtiles::MbtilesPool;
    /// use std::sync::Arc;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let pool = Arc::new(MbtilesPool::open_readonly("tiles.mbtiles").await?);
    ///
    /// // Can be called concurrently from multiple tasks
    /// let pool1 = Arc::clone(&pool);
    /// let handle1 = tokio::spawn(async move {
    ///     pool1.get_tile(4, 5, 6).await
    /// });
    ///
    /// let pool2 = Arc::clone(&pool);
    /// let handle2 = tokio::spawn(async move {
    ///     pool2.get_tile(4, 5, 7).await
    /// });
    ///
    /// let tile1 = handle1.await??;
    /// let tile2 = handle2.await??;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_tile(&self, z: u8, x: u32, y: u32) -> MbtResult<Option<Vec<u8>>> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles.get_tile(&mut *conn, z, x, y).await
    }

    /// Get a tile from the pool
    ///
    /// See [`MbtilesPool::get_tile`] if you don't need the tiles' hash.
    pub async fn get_tile_and_hash(
        &self,
        mbt_type: MbtType,
        z: u8,
        x: u32,
        y: u32,
    ) -> MbtResult<Option<(Vec<u8>, Option<String>)>> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles
            .get_tile_and_hash(&mut conn, mbt_type, z, x, y)
            .await
    }
    /// Check if a tile exists in the database.
    ///
    /// This method is slightly faster than [`Mbtiles::get_tile_and_hash`] and [`Mbtiles::get_tile`]
    /// because it only checks if the tile exists but does not retrieve tile data.
    /// Most of the time you would want to use the other two functions.
    pub async fn contains(&self, mbt_type: MbtType, z: u8, x: u32, y: u32) -> MbtResult<bool> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles.contains(&mut conn, mbt_type, z, x, y).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::temp_named_mbtiles;

    #[tokio::test]
    async fn test_metadata_invalid() {
        let script = include_str!("../../tests/fixtures/mbtiles/webp.sql");
        let (_mbt, _conn, file) = temp_named_mbtiles("test_metadata_invalid", script).await;

        let pool = MbtilesPool::open_readonly(file).await.unwrap();
        // invalid type
        assert!(pool.detect_type().await.is_err());
        let metadata = pool.get_metadata().await.unwrap();
        insta::assert_yaml_snapshot!(metadata, @r#"
        id: "file:test_metadata_invalid?mode=memory&cache=shared"
        tile_info:
          format: webp
          encoding: ""
        layer_type: baselayer
        tilejson:
          tilejson: 3.0.0
          tiles: []
          bounds:
            - -180
            - -85.05113
            - 180
            - 85.05113
          center:
            - 0
            - 0
            - 0
          maxzoom: 0
          minzoom: 0
          name: ne2sr
          format: webp
        "#);
    }

    #[tokio::test]
    async fn test_contains_invalid() {
        let script = include_str!("../../tests/fixtures/mbtiles/webp.sql");
        let (_mbt, _conn, file) = temp_named_mbtiles("test_contains_invalid", script).await;

        let pool = MbtilesPool::open_readonly(file).await.unwrap();
        assert!(pool.detect_type().await.is_err());

        assert!(pool.contains(MbtType::Flat, 0, 0, 0).await.unwrap());
        for error_mbt_type in [
            MbtType::Normalized { hash_view: false },
            MbtType::Normalized { hash_view: true },
            MbtType::FlatWithHash,
        ] {
            assert!(pool.contains(error_mbt_type, 0, 0, 0).await.is_err());
        }
    }

    #[tokio::test]
    async fn test_invalid_type() {
        let script = include_str!("../../tests/fixtures/mbtiles/webp.sql");
        let (_mbt, _conn, file) = temp_named_mbtiles("test_invalid_type", script).await;

        let pool = MbtilesPool::open_readonly(file).await.unwrap();

        // invalid type => cannot hash properly, but can get tile
        assert!(pool.detect_type().await.is_err());
        let t1 = pool.get_tile(0, 0, 0).await.unwrap().unwrap();
        assert!(!t1.is_empty());
        // this is an access and then md5 hash => should not fail
        let (t2, h2) = pool
            .get_tile_and_hash(MbtType::Flat, 0, 0, 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t2, t1);
        assert_eq!(h2, None);
        for error_types in [
            MbtType::FlatWithHash,
            MbtType::Normalized { hash_view: false },
            MbtType::Normalized { hash_view: true },
        ] {
            assert!(pool.get_tile_and_hash(error_types, 0, 0, 0).await.is_err());
        }
    }

    #[tokio::test]
    async fn test_metadata_normalized() {
        let script = include_str!("../../tests/fixtures/mbtiles/geography-class-png-no-bounds.sql");
        let (_mbt, _conn, file) = temp_named_mbtiles("test_metadata_normalized", script).await;

        let pool = MbtilesPool::open_readonly(file).await.unwrap();
        assert_eq!(
            pool.detect_type().await.unwrap(),
            MbtType::Normalized { hash_view: false }
        );
        let metadata = pool.get_metadata().await.unwrap();
        insta::assert_yaml_snapshot!(metadata, @r#"
        id: "file:test_metadata_normalized?mode=memory&cache=shared"
        tile_info:
          format: png
          encoding: ""
        tilejson:
          tilejson: 3.0.0
          tiles: []
          description: "One of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips. "
          legend: "<div style=\"text-align:center;\">\n\n<div style=\"font:12pt/16pt Georgia,serif;\">Geography Class</div>\n<div style=\"font:italic 10pt/16pt Georgia,serif;\">by MapBox</div>\n\n<img src=\"data:image/png;base64,iVBORw0KGgo\">\n</div>"
          maxzoom: 1
          minzoom: 0
          name: Geography Class
          template: "{{#__location__}}{{/__location__}}{{#__teaser__}}<div style=\"text-align:center;\">\n\n<img src=\"data:image/png;base64,{{flag_png}}\" style=\"-moz-box-shadow:0px 1px 3px #222;-webkit-box-shadow:0px 1px 5px #222;box-shadow:0px 1px 3px #222;\"><br>\n<strong>{{admin}}</strong>\n\n</div>{{/__teaser__}}{{#__full__}}{{/__full__}}"
          version: 1.0.0
        "#);
    }

    #[tokio::test]
    async fn test_contains_normalized() {
        let script = include_str!("../../tests/fixtures/mbtiles/geography-class-png-no-bounds.sql");
        let (_mbt, _conn, file) = temp_named_mbtiles("test_contains_normalized", script).await;

        let pool = MbtilesPool::open_readonly(file).await.unwrap();
        assert_eq!(
            pool.detect_type().await.unwrap(),
            MbtType::Normalized { hash_view: false }
        );

        for working_mbt_type in [
            MbtType::Normalized { hash_view: false },
            MbtType::Normalized { hash_view: true },
            MbtType::Flat,
        ] {
            assert!(pool.contains(working_mbt_type, 0, 0, 0).await.unwrap());
        }
        assert!(pool.contains(MbtType::FlatWithHash, 0, 0, 0).await.is_err());
    }

    #[tokio::test]
    async fn test_normalized() {
        let script = include_str!("../../tests/fixtures/mbtiles/geography-class-png-no-bounds.sql");
        let (_mbt, _conn, file) = temp_named_mbtiles("test_normalized", script).await;

        let pool = MbtilesPool::open_readonly(file).await.unwrap();
        assert_eq!(
            pool.detect_type().await.unwrap(),
            MbtType::Normalized { hash_view: false }
        );

        let t1 = pool.get_tile(0, 0, 0).await.unwrap().unwrap();
        assert!(!t1.is_empty());

        let (t2, h2) = pool
            .get_tile_and_hash(MbtType::Normalized { hash_view: false }, 0, 0, 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t2, t1);
        let expected_hash = Some("1578fdca522831a6435f7795586c235b".to_string());
        assert_eq!(h2, expected_hash);

        let (t3, h3) = pool
            .get_tile_and_hash(MbtType::Flat, 0, 0, 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t3, t2);
        assert_eq!(h3, None);
        for error_types in [
            MbtType::FlatWithHash,
            MbtType::Normalized { hash_view: true },
        ] {
            assert!(pool.get_tile_and_hash(error_types, 0, 0, 0).await.is_err());
        }
    }

    #[expect(clippy::too_many_lines)]
    #[tokio::test]
    async fn test_metadata_flat_with_hash() {
        let script = include_str!("../../tests/fixtures/mbtiles/zoomed_world_cities.sql");
        let (_mbt, _conn, file) = temp_named_mbtiles("test_metadata_flat_with_hash", script).await;

        let pool = MbtilesPool::open_readonly(file).await.unwrap();
        assert_eq!(pool.detect_type().await.unwrap(), MbtType::FlatWithHash);
        let metadata = pool.get_metadata().await.unwrap();
        insta::assert_yaml_snapshot!(metadata, @r#"
        id: "file:test_metadata_flat_with_hash?mode=memory&cache=shared"
        tile_info:
          format: mvt
          encoding: gzip
        layer_type: overlay
        tilejson:
          tilejson: 3.0.0
          tiles: []
          vector_layers:
            - id: cities
              fields:
                name: String
              description: ""
              maxzoom: 6
              minzoom: 0
          bounds:
            - -123.12359
            - -37.818085
            - 174.763027
            - 59.352706
          center:
            - -75.9375
            - 38.788894
            - 6
          description: Major cities from Natural Earth data
          maxzoom: 6
          minzoom: 0
          name: Major cities from Natural Earth data
          version: "2"
          format: pbf
        json:
          tilestats:
            layerCount: 1
            layers:
              - attributeCount: 1
                attributes:
                  - attribute: name
                    count: 68
                    type: string
                    values:
                      - Addis Ababa
                      - Amsterdam
                      - Athens
                      - Atlanta
                      - Auckland
                      - Baghdad
                      - Bangalore
                      - Bangkok
                      - Beijing
                      - Berlin
                      - Bogota
                      - Buenos Aires
                      - Cairo
                      - Cape Town
                      - Caracas
                      - Casablanca
                      - Chengdu
                      - Chicago
                      - Dakar
                      - Denver
                      - Dubai
                      - Geneva
                      - Hong Kong
                      - Houston
                      - Istanbul
                      - Jakarta
                      - Johannesburg
                      - Kabul
                      - Kiev
                      - Kinshasa
                      - Kolkata
                      - Lagos
                      - Lima
                      - London
                      - Los Angeles
                      - Madrid
                      - Manila
                      - Melbourne
                      - Mexico City
                      - Miami
                      - Monterrey
                      - Moscow
                      - Mumbai
                      - Nairobi
                      - New Delhi
                      - New York
                      - Paris
                      - Rio de Janeiro
                      - Riyadh
                      - Rome
                      - San Francisco
                      - Santiago
                      - Seoul
                      - Shanghai
                      - Singapore
                      - Stockholm
                      - Sydney
                      - São Paulo
                      - Taipei
                      - Tashkent
                      - Tehran
                      - Tokyo
                      - Toronto
                      - Vancouver
                      - Vienna
                      - "Washington, D.C."
                      - Ürümqi
                      - Ōsaka
                count: 68
                geometry: Point
                layer: cities
        agg_tiles_hash: D4E1030D57751A0B45A28A71267E46B8
        "#);
    }

    #[tokio::test]
    async fn test_contains_flat_with_hash() {
        let script = include_str!("../../tests/fixtures/mbtiles/zoomed_world_cities.sql");
        let (_mbt, _conn, file) = temp_named_mbtiles("test_contains_flat_with_hash", script).await;

        let pool = MbtilesPool::open_readonly(file).await.unwrap();
        assert_eq!(pool.detect_type().await.unwrap(), MbtType::FlatWithHash);
        for working_mbt_type in [MbtType::FlatWithHash, MbtType::Flat] {
            assert!(pool.contains(working_mbt_type, 6, 38, 19).await.unwrap());
        }
        for error_mbt_type in [
            MbtType::Normalized { hash_view: false },
            MbtType::Normalized { hash_view: true },
        ] {
            assert!(pool.contains(error_mbt_type, 6, 38, 19).await.is_err());
        }
    }

    #[tokio::test]
    async fn test_flat_with_hash() {
        let script = include_str!("../../tests/fixtures/mbtiles/zoomed_world_cities.sql");
        let (_mbt, _conn, file) = temp_named_mbtiles("test_flat_with_hash", script).await;

        let pool = MbtilesPool::open_readonly(file).await.unwrap();
        assert_eq!(pool.detect_type().await.unwrap(), MbtType::FlatWithHash);
        let t1 = pool.get_tile(6, 38, 19).await.unwrap().unwrap();
        assert!(!t1.is_empty());

        let (t2, h2) = pool
            .get_tile_and_hash(MbtType::FlatWithHash, 6, 38, 19)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t2, t1);
        let expected_hash = Some("80EE46337AC006B6BD14B4FA4D6E2EF9".to_string());
        assert_eq!(h2, expected_hash);
        let (t3, h3) = pool
            .get_tile_and_hash(MbtType::Flat, 6, 38, 19)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t3, t1);
        assert_eq!(h3, None);
        let (t3, h3) = pool
            .get_tile_and_hash(MbtType::Normalized { hash_view: true }, 6, 38, 19)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t3, t1);
        assert_eq!(h3, expected_hash);

        // no map table
        assert!(
            pool.get_tile_and_hash(MbtType::Normalized { hash_view: false }, 0, 0, 0)
                .await
                .is_err()
        );
    }
}
