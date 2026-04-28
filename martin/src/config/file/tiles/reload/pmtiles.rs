use std::collections::{BTreeMap, HashSet};
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::num::NonZeroU64;
use std::path::PathBuf;
use std::time::Duration;

use futures::future::try_join_all;
use futures::stream::TryStreamExt as _;
use object_store::{ObjectStore as _, ObjectStoreExt as _};
use tokio::time::{Instant, MissedTickBehavior};
use url::Url;

use crate::config::file::{
    CachePolicy, ConfigFileError, FileConfigEnum, FileConfigSrc, TileSourceConfiguration as _,
    tiles::pmtiles::PmtConfig,
};
use crate::config::primitives::{IdResolver, OptOneMany};
use crate::reload::ReloadAdvisory;
use crate::tile_source_manager::TileSourceManager;
use crate::{MartinError, MartinResult};

const PMTILES_EXTENSION: &str = ".pmtiles";

/// A periodic, polling-based reloader for `PMTiles` sources.
///
/// On each tick (default every 600 s; configured via `PmtConfig::reload_interval_secs`) it:
/// 1. Lists every directory in `pmtiles.paths` via `object_store`'s local backend, picking up
///    `*.pmtiles` files. Each listed object's `ObjectMeta::e_tag` becomes its version.
/// 2. Issues a HEAD against every individually-configured source in `pmtiles.sources` (any URL
///    or local file) to refresh its `ETag`.
/// 3. Diffs the resulting `id -> ETag` map against the previous tick via
///    [`ReloadAdvisory::from_maps`] and hands the advisory to
///    [`TileSourceManager::apply_changes`].
///
/// Inter-tick safety net: `pmtiles` 0.23 records each source's `data_version_string` at
/// construction time and returns `PmtError::SourceModified` from `get_tile` if the underlying
/// blob changes. The reloader then swaps in a fresh source on its next tick.
pub struct PMTilesReloader {
    id_resolver: IdResolver,
    tile_source_manager: TileSourceManager,
    pmt_config: PmtConfig,
    prefixes: Vec<PathBuf>,
    tracked_singles: BTreeMap<String, FileConfigSrc>,
    sources: BTreeMap<String, TrackedSource>,
}

#[derive(Clone, Debug)]
struct TrackedSource {
    url: Url,
    etag: EtagKey,
    cache: CachePolicy,
}

/// Wraps an `ETag` string as a `Copy` value usable with [`ReloadAdvisory::from_maps`].
///
/// `None` means the backend did not expose an `ETag`; two `None`s compare equal so missing
/// `ETag` info never triggers spurious updates here. Such sources rely instead on the in-flight
/// `PmtError::SourceModified` detection in `pmtiles` 0.23.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
struct EtagKey(Option<NonZeroU64>);

impl EtagKey {
    const UNKNOWN: Self = Self(None);

    fn from_etag(etag: Option<&str>) -> Self {
        match etag {
            None | Some("") => Self::UNKNOWN,
            Some(s) => {
                let mut h = DefaultHasher::new();
                s.hash(&mut h);
                Self(NonZeroU64::new(h.finish()).or(NonZeroU64::new(1)))
            }
        }
    }
}

impl PMTilesReloader {
    /// Builds a reloader from the post-`finalize` / post-`resolve` `PMTiles` config.
    ///
    /// Snapshots `pmtiles.paths` (local directories or URL-shaped prefixes such as
    /// `s3://bucket/`) and `pmtiles.sources`. Source `ETag`s are discovered lazily on the
    /// first tick.
    #[must_use]
    pub fn new(
        tsm: TileSourceManager,
        id_resolver: IdResolver,
        config: &FileConfigEnum<PmtConfig>,
    ) -> Self {
        let mut prefixes: Vec<PathBuf> = vec![];
        let mut tracked_singles: BTreeMap<String, FileConfigSrc> = BTreeMap::new();
        let mut pmt_config = PmtConfig::default();

        match config {
            FileConfigEnum::Config(cfg) => {
                pmt_config = cfg.custom.clone();
                match &cfg.paths {
                    OptOneMany::One(p) => prefixes.push(p.clone()),
                    OptOneMany::Many(ps) => prefixes.extend(ps.iter().cloned()),
                    OptOneMany::NoVals => {}
                }
                if let Some(srcs) = &cfg.sources {
                    for (id, src) in srcs {
                        tracked_singles.insert(id.clone(), src.clone());
                    }
                }
            }
            // After `resolve_files`, an empty-but-watched directory collapses back to
            // `Path` / `Paths` (no sources, one or more paths) rather than `Config`.
            FileConfigEnum::Path(p) => prefixes.push(p.clone()),
            FileConfigEnum::Paths(ps) => prefixes.extend(ps.iter().cloned()),
            FileConfigEnum::None => {}
        }

        prefixes.sort();
        prefixes.dedup();

        Self {
            tile_source_manager: tsm,
            id_resolver,
            pmt_config,
            prefixes,
            tracked_singles,
            sources: BTreeMap::new(),
        }
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(self.pmt_config.reload_interval_secs)
    }

    /// Spawns the polling loop. Returns immediately.
    ///
    /// No-op if the reload interval is zero or there is nothing to watch.
    pub fn start(mut self) {
        let interval = self.interval();
        if interval.is_zero() {
            tracing::info!("PMTilesReloader disabled (reload_interval_secs = 0)");
            return;
        }
        if self.prefixes.is_empty() && self.tracked_singles.is_empty() {
            return;
        }

        let mut tsm = self.tile_source_manager.clone();
        tokio::spawn(async move {
            match self.discover_sources().await {
                Ok(initial) => self.sources = initial,
                Err(e) => tracing::warn!("initial pmtiles discovery failed: {e:?}"),
            }

            let start = Instant::now() + interval;
            let mut ticker = tokio::time::interval_at(start, interval);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

            loop {
                ticker.tick().await;
                self.tick(&mut tsm).await;
            }
        });
    }

    /// Polls every prefix and every individually-configured source in parallel and returns
    /// the current `id -> TrackedSource` map. Failures of individual lookups are logged and
    /// treated as removals (except transient HEAD failures which preserve the previous version).
    async fn discover_sources(&self) -> MartinResult<BTreeMap<String, TrackedSource>> {
        let prefix_results =
            try_join_all(self.prefixes.iter().map(|p| self.list_prefix(p))).await?;

        let mut out: BTreeMap<String, TrackedSource> = BTreeMap::new();
        let mut seen_urls: HashSet<String> = HashSet::new();
        for entries in prefix_results {
            for (id, ts) in entries {
                seen_urls.insert(ts.url.to_string());
                out.insert(id, ts);
            }
        }

        let single_results = futures::future::join_all(
            self.tracked_singles
                .iter()
                .map(|(id, src)| self.head_single(id, src, &seen_urls)),
        )
        .await;
        for entry in single_results.into_iter().flatten() {
            let (id, ts) = entry;
            out.insert(id, ts);
        }

        Ok(out)
    }

    async fn list_prefix(
        &self,
        prefix_path: &PathBuf,
    ) -> MartinResult<Vec<(String, TrackedSource)>> {
        // Accept either a URL-shaped string (s3://bucket/path/) or an absolute local path.
        let Some(url) = prefix_path
            .to_str()
            .and_then(|s| Url::parse(s).ok())
            .or_else(|| Url::from_file_path(prefix_path).ok())
        else {
            tracing::warn!(
                "prefix {prefix_path:?} is neither a URL nor an absolute path; skipping"
            );
            return Ok(Vec::new());
        };
        let (store, base) = object_store::parse_url_opts(&url, &self.pmt_config.options)
            .map_err(|e| ConfigFileError::ObjectStoreUrlParsing(e, url.to_string()))?;

        let mut out = Vec::new();
        let mut stream = store.list(Some(&base));
        loop {
            let meta = match stream.try_next().await {
                Ok(Some(m)) => m,
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!("list failed for {url}: {e}; skipping prefix this tick");
                    break;
                }
            };
            if !meta.location.as_ref().ends_with(PMTILES_EXTENSION) {
                continue;
            }
            let stem = meta
                .location
                .filename()
                .and_then(|f| f.strip_suffix(PMTILES_EXTENSION))
                .unwrap_or("_unknown");
            // `meta.location` is reported relative to the *store's* root (which is `/` for
            // the local backend and the bucket for s3/gs/azure), not relative to the prefix
            // we asked to list. Reconstruct the absolute URL from scheme + authority +
            // location so it round-trips through `new_sources_url`.
            let object_url_str = format!(
                "{}://{}/{}",
                url.scheme(),
                url.host_str().unwrap_or(""),
                meta.location
            );
            let Ok(object_url) = Url::parse(&object_url_str) else {
                tracing::warn!("cannot build absolute URL from {object_url_str}");
                continue;
            };
            let id = self.id_resolver.resolve(stem, object_url.to_string());
            out.push((
                id,
                TrackedSource {
                    url: object_url,
                    etag: EtagKey::from_etag(meta.e_tag.as_deref()),
                    cache: CachePolicy::default(),
                },
            ));
        }
        Ok(out)
    }

    async fn head_single(
        &self,
        id: &str,
        src: &FileConfigSrc,
        seen_urls: &HashSet<String>,
    ) -> Option<(String, TrackedSource)> {
        let path = src.get_path();
        let Some(url) = path
            .to_str()
            .and_then(|s| Url::parse(s).ok())
            .or_else(|| Url::from_file_path(path).ok())
        else {
            tracing::warn!("cannot resolve URL for source {id} ({path:?})");
            return None;
        };
        if seen_urls.contains(url.as_str()) {
            return None;
        }
        let (store, key) = match object_store::parse_url_opts(&url, &self.pmt_config.options) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("object_store parse failed for {url}: {e}");
                return None;
            }
        };
        let etag = match store.head(&key).await {
            Ok(meta) => EtagKey::from_etag(meta.e_tag.as_deref()),
            Err(e) => {
                // Preserve previous etag so a transient HEAD failure does not flip the version.
                tracing::warn!("HEAD failed for {id} ({url}): {e}");
                self.sources.get(id).map_or(EtagKey::UNKNOWN, |s| s.etag)
            }
        };
        Some((
            id.to_string(),
            TrackedSource {
                url,
                etag,
                cache: src.cache_zoom(),
            },
        ))
    }

    async fn tick(&mut self, tsm: &mut TileSourceManager) {
        let next = match self.discover_sources().await {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("PMTilesReloader: discovery failed: {e:?}");
                return;
            }
        };

        let prev_versions: BTreeMap<String, EtagKey> = self
            .sources
            .iter()
            .map(|(k, v)| (k.clone(), v.etag))
            .collect();
        let next_versions: BTreeMap<String, EtagKey> =
            next.iter().map(|(k, v)| (k.clone(), v.etag)).collect();

        let next_clone = next.clone();
        let pmt_config = self.pmt_config.clone();

        let adv = ReloadAdvisory::from_maps(&prev_versions, &next_versions, async move |id| {
            let entry = next_clone.get(&id).ok_or_else(|| {
                MartinError::from(ConfigFileError::InvalidSourceFilePath(
                    id.clone(),
                    PathBuf::new(),
                ))
            })?;
            // Each call increments the global pmtiles cache id, so directory-cache entries
            // from the prior PmtilesSource are unreachable from the new one. They age out via
            // the moka TTL configured in `pmtiles.directory_cache.expiry`; without an expiry
            // they remain in memory until the process exits.
            pmt_config
                .new_sources_url(id, entry.url.clone(), entry.cache)
                .await
        })
        .await;

        match tsm.apply_changes(adv).await {
            Ok(()) => self.sources = next,
            Err(e) => tracing::warn!("PMTilesReloader: apply_changes failed: {e:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use insta::assert_yaml_snapshot;
    use rstest::rstest;

    use super::*;
    use crate::config::file::tiles::pmtiles::DEFAULT_RELOAD_INTERVAL_SECS;
    use crate::config::file::{FileConfig, FileConfigSource, OnInvalid};
    use crate::config::primitives::OptOneMany;

    fn make_reloader(config: &FileConfigEnum<PmtConfig>) -> PMTilesReloader {
        let tsm = TileSourceManager::new(None, OnInvalid::Warn);
        let resolver = IdResolver::new(&[]);
        PMTilesReloader::new(tsm, resolver, config)
    }

    #[rstest]
    #[case::none_none(None, None, true)]
    #[case::none_empty(None, Some(""), true)]
    #[case::empty_empty(Some(""), Some(""), true)]
    #[case::same_etag(Some("v1"), Some("v1"), true)]
    #[case::different_etags(Some("abc"), Some("xyz"), false)]
    #[case::known_vs_unknown(Some("anything"), None, false)]
    fn etag_key_equality(
        #[case] left: Option<&str>,
        #[case] right: Option<&str>,
        #[case] expected_eq: bool,
    ) {
        let l = EtagKey::from_etag(left);
        let r = EtagKey::from_etag(right);
        assert_eq!(
            l == r,
            expected_eq,
            "EtagKey({left:?}) vs EtagKey({right:?})"
        );
    }

    #[test]
    fn new_with_none_config_yields_default_interval() {
        let reloader = make_reloader(&FileConfigEnum::None);
        assert!(reloader.prefixes.is_empty());
        assert!(reloader.tracked_singles.is_empty());
        assert_eq!(
            reloader.interval(),
            Duration::from_secs(DEFAULT_RELOAD_INTERVAL_SECS)
        );
    }

    #[derive(serde::Serialize)]
    struct ReloaderSnapshot {
        prefix_count: usize,
        single_ids: Vec<String>,
        interval_secs: u64,
    }

    impl From<&PMTilesReloader> for ReloaderSnapshot {
        fn from(r: &PMTilesReloader) -> Self {
            Self {
                prefix_count: r.prefixes.len(),
                single_ids: r.tracked_singles.keys().cloned().collect(),
                interval_secs: r.interval().as_secs(),
            }
        }
    }

    #[test]
    fn new_extracts_paths_and_sources() {
        let mut sources: BTreeMap<String, FileConfigSrc> = BTreeMap::new();
        sources.insert(
            "src_a".to_string(),
            FileConfigSrc::Obj(FileConfigSource {
                path: PathBuf::from("/tmp/a.pmtiles"),
                cache: CachePolicy::default(),
            }),
        );
        let cfg = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::Many(vec![PathBuf::from("/tmp/dir1"), PathBuf::from("/tmp/dir2")]),
            sources: Some(sources),
            custom: PmtConfig {
                reload_interval_secs: 30,
                ..PmtConfig::default()
            },
        });

        assert_yaml_snapshot!(ReloaderSnapshot::from(&make_reloader(&cfg)), @r"
        prefix_count: 2
        single_ids:
          - src_a
        interval_secs: 30
        ");
    }

    #[test]
    fn new_dedups_prefixes() {
        let cfg = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::Many(vec![PathBuf::from("/tmp/dir"), PathBuf::from("/tmp/dir")]),
            sources: None,
            custom: PmtConfig::default(),
        });
        assert_yaml_snapshot!(ReloaderSnapshot::from(&make_reloader(&cfg)), @r"
        prefix_count: 1
        single_ids: []
        interval_secs: 600
        ");
    }
}

/// Integration tests using a `MinIO` testcontainer alongside a local-file prefix to exercise
/// `discover_sources` end-to-end across both backends. Requires Docker.
#[cfg(test)]
mod minio_tests {
    use std::collections::HashMap;

    use insta::assert_yaml_snapshot;
    use object_store::PutPayload;
    use object_store::path::Path as ObjPath;
    use tempfile::tempdir;
    use testcontainers_modules::minio::MinIO;
    use testcontainers_modules::testcontainers::core::{CmdWaitFor, ExecCommand};
    use testcontainers_modules::testcontainers::runners::AsyncRunner as _;

    use super::*;
    use crate::config::file::{FileConfig, OnInvalid};

    fn sorted_ids(map: &BTreeMap<String, TrackedSource>) -> Vec<String> {
        map.keys().cloned().collect()
    }

    const BUCKET: &str = "pmt-bucket";
    /// A small valid `.pmtiles` blob (47 KB) used for upload payloads.
    const FIXTURE_A: &[u8] =
        include_bytes!("../../../../../../tests/fixtures/pmtiles2/webp2.pmtiles");
    /// A different valid `.pmtiles` blob (~717 KB) used to force an `ETag` change on re-upload.
    const FIXTURE_B: &[u8] = include_bytes!("../../../../../../tests/fixtures/pmtiles/png.pmtiles");

    fn s3_options(endpoint: &str) -> HashMap<String, String> {
        let mut o = HashMap::new();
        o.insert("aws_endpoint".into(), endpoint.to_string());
        o.insert("aws_access_key_id".into(), "minioadmin".into());
        o.insert("aws_secret_access_key".into(), "minioadmin".into());
        o.insert("aws_region".into(), "us-east-1".into());
        o.insert("allow_http".into(), "true".into());
        o.insert("virtual_hosted_style_request".into(), "false".into());
        o
    }

    #[tokio::test]
    async fn lists_etag_changes_and_removals_across_s3_and_local() {
        // MinIO maps subdirectories of /data to buckets, so creating the directory creates
        // the bucket without needing an mc client or signed PUT request.
        let minio = MinIO::default().start().await.unwrap();
        minio
            .exec(
                ExecCommand::new(["mkdir", &format!("/data/{BUCKET}")])
                    .with_cmd_ready_condition(CmdWaitFor::exit()),
            )
            .await
            .unwrap();

        let host = minio.get_host().await.unwrap();
        let port = minio.get_host_port_ipv4(9000).await.unwrap();
        let endpoint = format!("http://{host}:{port}");
        let options = s3_options(&endpoint);

        let s3_url: Url = format!("s3://{BUCKET}/").parse().unwrap();
        let (s3_store, _base) = object_store::parse_url_opts(&s3_url, &options).unwrap();
        s3_store
            .put(
                &ObjPath::from("a.pmtiles"),
                PutPayload::from_static(FIXTURE_A),
            )
            .await
            .unwrap();
        s3_store
            .put(
                &ObjPath::from("b.pmtiles"),
                PutPayload::from_static(FIXTURE_A),
            )
            .await
            .unwrap();

        let local_dir = tempdir().unwrap();
        let local_path = local_dir.path().join("local.pmtiles");
        std::fs::write(&local_path, FIXTURE_A).unwrap();

        // The interval is non-zero so the reloader is not "disabled," but we drive
        // `discover_sources` manually rather than letting the spawned loop fire.
        let cfg = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::Many(vec![
                local_dir.path().to_path_buf(),
                PathBuf::from(format!("s3://{BUCKET}/")),
            ]),
            sources: None,
            custom: PmtConfig {
                reload_interval_secs: 1,
                options: options.clone(),
                ..PmtConfig::default()
            },
        });
        let tsm = TileSourceManager::new(None, OnInvalid::Warn);
        let resolver = IdResolver::new(&[]);
        let reloader = PMTilesReloader::new(tsm, resolver, &cfg);

        let initial = reloader.discover_sources().await.unwrap();
        assert_yaml_snapshot!(sorted_ids(&initial), @r"
        - a
        - b
        - local
        ");
        for (id, ts) in &initial {
            assert_ne!(ts.etag, EtagKey::UNKNOWN, "source {id} should have an ETag");
        }
        let etag_a_v1 = initial["a"].etag;
        let etag_local_v1 = initial["local"].etag;

        s3_store
            .put(
                &ObjPath::from("a.pmtiles"),
                PutPayload::from_static(FIXTURE_B),
            )
            .await
            .unwrap();
        let after_update = reloader.discover_sources().await.unwrap();
        assert_ne!(
            after_update["a"].etag, etag_a_v1,
            "ETag for a.pmtiles should change after re-upload"
        );
        assert_eq!(
            after_update["b"].etag, initial["b"].etag,
            "ETag for unchanged b.pmtiles should be stable"
        );
        assert_eq!(
            after_update["local"].etag, etag_local_v1,
            "ETag for unchanged local file should be stable"
        );

        s3_store.delete(&ObjPath::from("b.pmtiles")).await.unwrap();
        std::fs::write(local_dir.path().join("local2.pmtiles"), FIXTURE_B).unwrap();
        let after_remove_and_add = reloader.discover_sources().await.unwrap();
        assert_yaml_snapshot!(sorted_ids(&after_remove_and_add), @r"
        - a
        - local
        - local2
        ");
    }
}
