use std::fmt::Debug;
use std::num::NonZeroU32;
use std::path::PathBuf;

use martin_core::tiles::BoxedSource;
use martin_core::tiles::geojson::source::GeoJsonSource;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::config::file::{
    CachePolicy, CollectUnrecognizedKeys, ConfigurationLivecycleHooks, SourceBuildResult,
    TileSourceConfiguration, UnrecognizedValues,
};

/// The MVT-spec tile extent `MapLibre` assumes, used when none is configured.
const fn default_extent() -> NonZeroU32 {
    NonZeroU32::new(4096).expect("4096 is non-zero")
}

const fn default_buffer() -> u32 {
    64
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires &T"
)]
fn is_default_extent(extent: &NonZeroU32) -> bool {
    *extent == default_extent()
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires &T"
)]
const fn is_default_buffer(buffer: &u32) -> bool {
    *buffer == default_buffer()
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    CollectUnrecognizedKeys,
    ConfigurationLivecycleHooks,
)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct GeoJsonConfig {
    /// Side length of the MVT tile coordinate grid each tile is encoded into, defaulting to 4096.
    #[serde(default = "default_extent", skip_serializing_if = "is_default_extent")]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &4096u32))]
    pub extent: NonZeroU32,

    /// Clip margin kept around each tile edge, in tile units, defaulting to 64.
    /// Increase it if you see seam artifacts on line caps/joins or polygon outlines near tile edges.
    #[serde(default = "default_buffer", skip_serializing_if = "is_default_buffer")]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &64u32))]
    pub buffer: u32,

    /// Whether `paths` are scanned recursively
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &false))]
    pub recursive: Option<bool>,
    /// Zoom-level bounds for caching the tiles of every `GeoJSON` source without its own `cache`.
    /// Overrides the top-level `cache` bounds.
    #[serde(default, skip_serializing_if = "CachePolicy::is_empty")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "crate::config::file::CachePolicyShape")
    )]
    pub cache: CachePolicy,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl Default for GeoJsonConfig {
    fn default() -> Self {
        Self {
            extent: default_extent(),
            buffer: default_buffer(),
            recursive: None,
            cache: CachePolicy::default(),
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

impl TileSourceConfiguration for GeoJsonConfig {
    fn parse_urls() -> bool {
        false
    }

    fn cache(&self) -> CachePolicy {
        self.cache
    }

    async fn new_sources(
        &self,
        id: String,
        path: PathBuf,
        cache: CachePolicy,
    ) -> SourceBuildResult<BoxedSource> {
        let geojson_source =
            GeoJsonSource::new(id, path, cache.zoom(), self.extent, self.buffer).await?;
        Ok(Box::new(geojson_source))
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "unreachable stub; async keeps it simple to write and read"
    )]
    async fn new_sources_url(
        &self,
        _id: String,
        _url: Url,
        _cache: CachePolicy,
    ) -> SourceBuildResult<BoxedSource> {
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
        CachePolicy, CollectUnrecognizedKeys as _, ConfigurationLivecycleHooks as _,
        FileConfigEnum, FileConfigSource, FileConfigSrc,
    };

    #[tokio::test]
    async fn parse() {
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
        cfg.finalize().await.unwrap();
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
                    "pm-src1".to_owned(),
                    FileConfigSrc::Path(PathBuf::from("/tmp/file.ext"))
                ),
                (
                    "pm-src2".to_owned(),
                    FileConfigSrc::Obj(Box::new(FileConfigSource {
                        path: PathBuf::from("/tmp/file.ext"),
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mlt: None,
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mvt: None,
                        #[cfg(all(feature = "hillshade", feature = "_tiles"))]
                        convert_to_hillshade: None,
                        #[cfg(all(feature = "contour", feature = "_tiles"))]
                        convert_to_contour: None,
                        cache: CachePolicy::default(),
                        cache_control: None,
                    }))
                ),
                (
                    "pm-src3".to_owned(),
                    FileConfigSrc::Path(PathBuf::from("https://example.org/file3.ext"))
                ),
                (
                    "pm-src4".to_owned(),
                    FileConfigSrc::Obj(Box::new(FileConfigSource {
                        path: PathBuf::from("https://example.org/file4.ext"),
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mlt: None,
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mvt: None,
                        #[cfg(all(feature = "hillshade", feature = "_tiles"))]
                        convert_to_hillshade: None,
                        #[cfg(all(feature = "contour", feature = "_tiles"))]
                        convert_to_contour: None,
                        cache: CachePolicy::default(),
                        cache_control: None,
                    }))
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
