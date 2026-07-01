use std::fmt::Debug;
use std::num::NonZeroU32;
use std::path::PathBuf;

use martin_core::tiles::BoxedSource;
use martin_core::tiles::geojson::source::GeoJsonSource;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::MartinResult;
use crate::config::file::{
    CachePolicy, ConfigurationLivecycleHooks, TileSourceConfiguration, UnrecognizedKeys,
    UnrecognizedValues,
};

/// The MVT-spec tile extent `MapLibre` assumes, used when none is configured.
fn default_extent() -> NonZeroU32 {
    NonZeroU32::new(4096).expect("4096 is non-zero")
}

const fn default_buffer() -> u32 {
    64
}

#[expect(clippy::trivially_copy_pass_by_ref, reason = "serde skip_serializing_if requires &T")]
fn is_default_extent(extent: &NonZeroU32) -> bool {
    *extent == default_extent()
}

#[expect(clippy::trivially_copy_pass_by_ref, reason = "serde skip_serializing_if requires &T")]
const fn is_default_buffer(buffer: &u32) -> bool {
    *buffer == default_buffer()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct GeoJsonConfig {
    /// Side length of the MVT tile coordinate grid each tile is encoded into, defaulting to 4096.
    #[serde(default = "default_extent", skip_serializing_if = "is_default_extent")]
    pub extent: NonZeroU32,

    /// Clip margin kept around each tile edge, in tile units, defaulting to 64.
    /// Increase it if you see seam artifacts on line caps/joins or polygon outlines near tile edges.
    #[serde(default = "default_buffer", skip_serializing_if = "is_default_buffer")]
    pub buffer: u32,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl Default for GeoJsonConfig {
    fn default() -> Self {
        Self {
            extent: default_extent(),
            buffer: default_buffer(),
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

impl ConfigurationLivecycleHooks for GeoJsonConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

impl TileSourceConfiguration for GeoJsonConfig {
    fn parse_urls() -> bool {
        false
    }

    async fn new_sources(
        &self,
        id: String,
        path: PathBuf,
        cache: CachePolicy,
    ) -> MartinResult<BoxedSource> {
        let geojson_source =
            GeoJsonSource::new(id, path, cache.zoom(), self.extent, self.buffer).await?;
        Ok(Box::new(geojson_source))
    }

    async fn new_sources_url(
        &self,
        _id: String,
        _url: Url,
        _cache: CachePolicy,
    ) -> MartinResult<BoxedSource> {
        unreachable!()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use indoc::indoc;

    use crate::config::file::geojson::GeoJsonConfig;
    use crate::config::file::{
        CachePolicy, ConfigurationLivecycleHooks as _, FileConfigEnum, FileConfigSource,
        FileConfigSrc,
    };

    #[test]
    fn parse() {
        let mut cfg = serde_saphyr::from_str::<FileConfigEnum<GeoJsonConfig>>(indoc! {"
            paths:
              - /dir-path
              - /path/to/file2.ext
              - http://example.org/file.ext
            sources:
                pm-src1: /tmp/file.ext
                pm-src2:
                  path: /tmp/file.ext
                pm-src3: https://example.org/file3.ext
                pm-src4:
                  path: https://example.org/file4.ext
        "})
        .unwrap();
        cfg.finalize().unwrap();
        let unrecognised = cfg.get_unrecognized_keys();
        assert!(
            unrecognised.is_empty(),
            "unrecognized config: {unrecognised:?}"
        );
        let FileConfigEnum::Config(cfg) = cfg else {
            panic!();
        };
        let paths = cfg.paths.clone().into_iter().collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/dir-path"),
                PathBuf::from("/path/to/file2.ext"),
                PathBuf::from("http://example.org/file.ext"),
            ]
        );
        assert_eq!(
            cfg.sources,
            Some(BTreeMap::from_iter(vec![
                (
                    "pm-src1".to_string(),
                    FileConfigSrc::Path(PathBuf::from("/tmp/file.ext"))
                ),
                (
                    "pm-src2".to_string(),
                    FileConfigSrc::Obj(FileConfigSource {
                        path: PathBuf::from("/tmp/file.ext"),
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mlt: None,
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mvt: None,
                        cache: CachePolicy::default(),
                    })
                ),
                (
                    "pm-src3".to_string(),
                    FileConfigSrc::Path(PathBuf::from("https://example.org/file3.ext"))
                ),
                (
                    "pm-src4".to_string(),
                    FileConfigSrc::Obj(FileConfigSource {
                        path: PathBuf::from("https://example.org/file4.ext"),
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mlt: None,
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mvt: None,
                        cache: CachePolicy::default(),
                    })
                ),
            ]))
        );
    }

    #[test]
    fn extent_and_buffer_default_to_4096_and_64() {
        let cfg = serde_saphyr::from_str::<GeoJsonConfig>("{}").unwrap();
        assert_eq!(cfg.extent.get(), 4096);
        assert_eq!(cfg.buffer, 64);
    }

    #[test]
    fn extent_and_buffer_are_overridable() {
        let cfg = serde_saphyr::from_str::<GeoJsonConfig>(indoc! {"
            extent: 2048
            buffer: 16
        "})
        .unwrap();
        assert_eq!(cfg.extent.get(), 2048);
        assert_eq!(cfg.buffer, 16);
    }

    #[test]
    fn zero_extent_is_rejected() {
        // `NonZeroU32` guards the divisor in the tile-coordinate transform.
        serde_saphyr::from_str::<GeoJsonConfig>("extent: 0").unwrap_err();
    }
}
