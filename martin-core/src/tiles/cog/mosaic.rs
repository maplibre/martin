//! Tile sources over a mosaic of many COGs, each covering one tile at the mosaic's file zoom.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cog_tiles::{AsyncCogMosaic, MosaicOptions};
use object_store::ObjectStore;
use object_store::path::Path;
use tilejson::tilejson;

use crate::CacheZoomRange;
use crate::tiles::cog::CogError;
use crate::tiles::cog::source::{CogSource, CogTiles};

/// How a mosaic of COGs is laid out and how many opened files it keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CogMosaicOptions {
    /// The zoom level at which each file covers exactly one tile, also the lowest zoom served.
    pub file_zoom: u8,
    /// The highest zoom served. `None` uses the highest zoom of the file opened at startup.
    pub max_zoom: Option<u8>,
    /// The most opened files (and known-missing ones) kept at once.
    pub max_cached_files: u64,
    /// How long an opened file is kept, so replaced or added files are picked up eventually.
    pub file_expiry: Duration,
    /// How long an opened file is kept without being used. `None` keeps it until `file_expiry`.
    pub file_idle_timeout: Option<Duration>,
}

impl CogMosaicOptions {
    fn mosaic(&self) -> MosaicOptions {
        MosaicOptions::new(self.file_zoom)
            .with_max_cached_files(self.max_cached_files)
            .with_file_expiry(self.file_expiry)
            .with_file_idle_timeout(self.file_idle_timeout)
    }
}

impl CogSource {
    /// Creates a source over a mosaic of local COGs under `root`, named by `template`
    /// (e.g. `{quadkey}.tif` or `{z}/{x}/{y}.tif`) relative to it.
    ///
    /// One file under `root` is opened at startup to learn the tile size and format;
    /// the others are opened on first use.
    pub async fn new_mosaic_dir(
        id: String,
        root: PathBuf,
        template: &str,
        options: CogMosaicOptions,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, CogError> {
        let location = root.join(template).display().to_string();
        let mosaic = AsyncCogMosaic::open_dir(root, template, options.mosaic()).await?;
        Self::from_mosaic(id, mosaic, &options, location, cache_zoom)
    }

    /// Creates a source over a mosaic of COGs under `prefix` in `store`, named by `template`
    /// (e.g. `{quadkey}.tif` or `{z}/{x}/{y}.tif`) relative to it.
    ///
    /// `location` is the display form of the mosaic (e.g. a URL without credentials), used in errors.
    /// The first object under `prefix` whose key matches `template` is opened at startup to learn
    /// the tile size and format, which needs permission to list the prefix.
    /// The others are opened on first use.
    pub async fn new_mosaic_object_store(
        id: String,
        store: Arc<dyn ObjectStore>,
        prefix: Path,
        template: &str,
        location: String,
        options: CogMosaicOptions,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, CogError> {
        let mosaic = AsyncCogMosaic::open_object_store(store, prefix, template, &location, options.mosaic()).await?;
        Self::from_mosaic(id, mosaic, &options, location, cache_zoom)
    }

    fn from_mosaic(
        id: String,
        mosaic: AsyncCogMosaic,
        options: &CogMosaicOptions,
        location: String,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, CogError> {
        let info = *mosaic.info();
        let tilejson = tilejson! {
            tiles: vec![],
            minzoom: info.file_zoom,
            maxzoom: options.max_zoom.unwrap_or(info.max_zoom),
        };
        Self::with_layout(
            id,
            CogTiles::Mosaic(Arc::new(mosaic)),
            tilejson,
            (info.tile_size, info.format),
            location,
            cache_zoom,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path as StdPath;
    use std::sync::Arc;
    use std::time::Duration;

    use martin_tile_utils::TileCoord;
    use object_store::memory::InMemory;
    use object_store::path::Path;
    use object_store::{ObjectStoreExt as _, PutPayload};
    use rstest::rstest;

    use crate::CacheZoomRange;
    use crate::tiles::Source as _;
    use crate::tiles::cog::{CogMosaicOptions, CogSource};

    const MOSAIC_DIR: &str = "../tests/fixtures/cog/mosaic";

    #[tokio::test]
    async fn a_directory_mosaic_serves_each_tile_from_its_file() {
        let source = CogSource::new_mosaic_dir(
            "mosaic".to_owned(),
            MOSAIC_DIR.into(),
            "{quadkey}.tif",
            CogMosaicOptions {
                file_zoom: 16,
                max_zoom: None,
                max_cached_files: 100,
                file_expiry: Duration::from_mins(1),
                file_idle_timeout: None,
            },
            CacheZoomRange::default(),
        )
        .await
        .unwrap();
        let tilejson = source.get_tilejson();
        assert_eq!((tilejson.minzoom, tilejson.maxzoom), (Some(16), Some(17)));
        assert_eq!(tilejson.other["tileSize"], 256);
        assert_eq!(tilejson.other["format"], "png");
        assert_eq!(tilejson.bounds, None);

        for (name, x, y) in [
            ("0313131311301120.tif", 32_748, 21_794),
            ("0313131311301121.tif", 32_749, 21_794),
            ("0313131311301122.tif", 32_748, 21_795),
            ("0313131311301123.tif", 32_749, 21_795),
        ] {
            let single = CogSource::new(
                "single".to_owned(),
                StdPath::new(MOSAIC_DIR).join(name),
                CacheZoomRange::default(),
            )
            .await
            .unwrap();
            for xyz in [
                TileCoord::new_unchecked(16, x, y),
                TileCoord::new_unchecked(17, x * 2 + 1, y * 2),
            ] {
                let tile = source.get_tile(xyz, None).await.unwrap();
                assert!(!tile.is_empty(), "{xyz}");
                assert_eq!(tile, single.get_tile(xyz, None).await.unwrap(), "{xyz}");
            }
        }
    }

    #[rstest]
    #[case::above_the_file_zoom(TileCoord::new_unchecked(15, 16_374, 10_897))]
    #[case::no_file_for_this_tile(TileCoord::new_unchecked(16, 32_750, 21_794))]
    #[case::below_a_missing_file(TileCoord::new_unchecked(17, 65_500, 43_588))]
    #[case::deeper_than_the_files_go(TileCoord::new_unchecked(18, 130_992, 87_176))]
    #[case::deeper_than_the_max_zoom(TileCoord::new_unchecked(19, 261_984, 174_352))]
    #[tokio::test]
    async fn tiles_without_a_file_are_empty(#[case] xyz: TileCoord) {
        let source = CogSource::new_mosaic_dir(
            "mosaic".to_owned(),
            MOSAIC_DIR.into(),
            "{quadkey}.tif",
            CogMosaicOptions {
                file_zoom: 16,
                max_zoom: Some(18),
                max_cached_files: 100,
                file_expiry: Duration::from_mins(1),
                file_idle_timeout: None,
            },
            CacheZoomRange::default(),
        )
        .await
        .unwrap();
        assert_eq!(source.get_tilejson().maxzoom, Some(18));
        assert!(source.get_tile(xyz, None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_object_store_mosaic_matches_a_directory_mosaic() {
        let store = Arc::new(InMemory::new());
        // Unrelated objects are skipped when looking for a file to probe.
        store
            .put(&Path::from("imagery/readme.txt"), PutPayload::from("hi"))
            .await
            .unwrap();
        for entry in std::fs::read_dir(MOSAIC_DIR).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy();
            let bytes = std::fs::read(&path).unwrap();
            let key = Path::from(format!("imagery/{name}"));
            store.put(&key, PutPayload::from(bytes)).await.unwrap();
        }
        let options = CogMosaicOptions {
            file_zoom: 16,
            max_zoom: None,
            max_cached_files: 100,
            file_expiry: Duration::from_mins(1),
            file_idle_timeout: None,
        };
        let remote = CogSource::new_mosaic_object_store(
            "remote".to_owned(),
            store,
            Path::from("imagery"),
            "{quadkey}.tif",
            "memory://imagery/{quadkey}.tif".to_owned(),
            options,
            CacheZoomRange::default(),
        )
        .await
        .unwrap();
        let local = CogSource::new_mosaic_dir(
            "local".to_owned(),
            MOSAIC_DIR.into(),
            "{quadkey}.tif",
            options,
            CacheZoomRange::default(),
        )
        .await
        .unwrap();

        assert_eq!(remote.get_tilejson(), local.get_tilejson());
        assert_eq!(remote.get_tile_info(), local.get_tile_info());
        let xyz = TileCoord::new_unchecked(17, 65_499, 43_589);
        let tile = remote.get_tile(xyz, None).await.unwrap();
        assert!(!tile.is_empty());
        assert_eq!(tile, local.get_tile(xyz, None).await.unwrap());
        let missing = TileCoord::new_unchecked(16, 32_750, 21_794);
        assert!(remote.get_tile(missing, None).await.unwrap().is_empty());
    }
}
