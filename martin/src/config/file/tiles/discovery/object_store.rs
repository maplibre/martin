//! Storage-neutral discovery over remote object prefixes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::stream::TryStreamExt as _;
use object_store::{ObjectStore as _, ObjectStoreExt as _};
use url::Url;

#[cfg(feature = "unstable-cog")]
use crate::config::file::cog::CogConfig;
#[cfg(feature = "pmtiles")]
use crate::config::file::pmtiles::PmtConfig;
use crate::config::file::process::{ProcessConfig, ResolvedProcess};
use crate::config::file::source_location::SourceLocation;
use crate::config::file::tiles::discovery::fs::per_source_process;
use crate::config::file::tiles::discovery::{BuiltSource, Discovered, Discovery, Version};
use crate::config::file::{
    CachePolicy, ConfigFileError, FileConfig, FileConfigSrc, SourceBuildResult,
    TileSourceConfiguration,
};
use crate::config::primitives::IdResolver;
use crate::reload::FileKind;

pub type ObjectStoreParser = Box<
    dyn Fn(
            &Url,
        )
            -> object_store::Result<(Box<dyn object_store::ObjectStore>, object_store::path::Path)>
        + Send
        + Sync,
>;

type PrefixEntry = (String, Url, Version);

/// Builds a source discovered in an object store.
///
/// The enum keeps the supported source kinds explicit and avoids erasing async builders behind
/// boxed, pinned futures. Future remote-backed source kinds can add a variant here.
pub enum ObjectStoreSourceBuilder {
    #[cfg(feature = "pmtiles")]
    Pmtiles(Box<PmtConfig>),
    #[cfg(feature = "unstable-cog")]
    Cog(Box<CogConfig>),
}

impl ObjectStoreSourceBuilder {
    async fn build(
        &self,
        id: String,
        url: Url,
        cache: CachePolicy,
    ) -> SourceBuildResult<BuiltSource> {
        match self {
            #[cfg(feature = "pmtiles")]
            Self::Pmtiles(config) => config
                .new_sources_url(id, url, cache)
                .await
                .map(|s| s.boxed().into()),
            #[cfg(feature = "unstable-cog")]
            Self::Cog(config) => config
                .new_sources_url(id, url, cache)
                .await
                .map(|s| s.boxed().into()),
        }
    }
}

/// One configured remote object that participates in conditional replacement detection.
#[derive(Clone)]
pub struct ConfiguredObject {
    /// The resolved source id, matching what startup resolution produced.
    id: String,
    /// The full object URL; credentials stay store-side and never reach diagnostics.
    url: Url,
    /// The per-source cache bounds, falling back to the kind-level policy.
    policy: CachePolicy,
    /// The per-source `convert_to_*` and `cache_control` overrides, if any.
    process: Option<ProcessConfig>,
    /// The configured entry, for `--save-config` provenance.
    src: FileConfigSrc,
}

/// A [`Discovery`] over the explicitly configured remote objects in the resolved `sources` map.
/// Object URLs supplied through `paths` are normalized into that map by startup resolution before
/// reloaders are constructed.
/// Each pass sends one `HEAD` per object and derives a [`Version`] from its `ETag` or
/// last-modified timestamp, so a replaced object is rebuilt while an unchanged one costs
/// nothing but the round-trip. A failed check retains the object's last-known version, so a
/// transient store outage cannot surface as a source removal.
pub struct ConfiguredObjectDiscovery {
    kind: FileKind,
    label: &'static str,
    objects: Vec<ConfiguredObject>,
    reload_interval: Duration,
    /// Last-known version per object id, for the retention-on-failure behavior.
    last_versions: Mutex<BTreeMap<String, Version>>,
    parser: ObjectStoreParser,
    build: ObjectStoreSourceBuilder,
    process: ResolvedProcess,
}

impl ConfiguredObjectDiscovery {
    #[expect(
        clippy::too_many_arguments,
        reason = "one call per source kind, and every argument is a distinct kind-level input"
    )]
    #[must_use]
    pub fn from_config<T>(
        kind: FileKind,
        config: &FileConfig<T>,
        label: &'static str,
        reload_interval: Duration,
        default_cache: CachePolicy,
        process: &ProcessConfig,
        parser: ObjectStoreParser,
        build: ObjectStoreSourceBuilder,
    ) -> Self {
        let mut objects = Vec::new();
        for (id, src) in &config.sources {
            let Ok(SourceLocation::ObjectStore(url) | SourceLocation::Http(url)) =
                SourceLocation::classify_path(src.get_path())
            else {
                // Local sources belong to the file-based discovery.
                continue;
            };
            objects.push(ConfiguredObject {
                id: id.clone(),
                url,
                policy: src.cache_zoom().or(default_cache),
                process: per_source_process(process, src),
                src: src.clone(),
            });
        }

        Self {
            kind,
            label,
            objects,
            reload_interval,
            last_versions: Mutex::default(),
            parser,
            build,
            process: process
                .resolve()
                .expect("the kind level carries no range-checked settings"),
        }
    }

    /// Whether any configured remote object participates in replacement detection.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    #[must_use]
    pub const fn reload_interval(&self) -> Duration {
        self.reload_interval
    }
}

/// On a failed check, keeps the object at its last-known version so the driver sees no change;
/// without one, drops the object for this tick so the driver sees nothing to build.
fn retain_or_skip(
    out: &mut BTreeMap<String, (Version, ConfiguredObject)>,
    label: &str,
    last: Option<Version>,
    object: &ConfiguredObject,
    error: &object_store::Error,
) {
    if let Some(version) = last {
        tracing::warn!(
            "{label}: check failed for {}: {error}; retaining the last-known version",
            sanitized_url(&object.url)
        );
        out.insert(object.id.clone(), (version, object.clone()));
    } else {
        tracing::warn!(
            "{label}: check failed for {}: {error}; skipping this tick",
            sanitized_url(&object.url)
        );
    }
}

impl Discovery for ConfiguredObjectDiscovery {
    type Args = ConfiguredObject;
    async fn discover(&self) -> SourceBuildResult<Discovered<Self::Args>> {
        let mut out: BTreeMap<String, (Version, Self::Args)> = BTreeMap::new();
        for object in &self.objects {
            let last = self
                .last_versions
                .lock()
                .expect("version map mutex")
                .get(&object.id)
                .copied();
            let (store, path) = match (self.parser)(&object.url) {
                Ok(parsed) => parsed,
                Err(error) => {
                    retain_or_skip(&mut out, self.label, last, object, &error);
                    continue;
                }
            };
            match store.head(&path).await {
                Ok(meta) => {
                    let version = version_from_meta(&meta);
                    self.last_versions
                        .lock()
                        .expect("version map mutex")
                        .insert(object.id.clone(), version);
                    out.insert(object.id.clone(), (version, object.clone()));
                }
                Err(error) => retain_or_skip(&mut out, self.label, last, object, &error),
            }
        }
        Ok(Discovered::new(out))
    }

    async fn build(&self, id: &str, args: &Self::Args) -> SourceBuildResult<BuiltSource> {
        let source = self
            .build
            .build(id.to_owned(), args.url.clone(), args.policy)
            .await?
            .source;
        BuiltSource::with_file_config(
            source,
            id,
            args.process.as_ref(),
            self.kind,
            args.src.clone(),
        )
    }

    fn process(&self) -> ResolvedProcess {
        self.process.clone()
    }
}

/// A [`Discovery`] over one or more remote object-store prefixes.
///
/// An object listed under a prefix that is also configured explicitly under `sources` is skipped,
/// so the explicit entry and its per-source settings are not replaced by a plain discovered copy.
pub struct ObjectStoreDiscovery {
    remote_prefixes: Vec<Url>,
    /// Sanitized URLs of the remote objects configured explicitly under `sources`, the same key
    /// the [`IdResolver`] tells objects apart by.
    configured: BTreeSet<String>,
    extensions: Arc<[String]>,
    label: &'static str,
    id_resolver: IdResolver,
    reload_interval: Duration,
    parser: ObjectStoreParser,
    /// Last successful listing per prefix, retained across transient listing failures.
    last_entries: Mutex<BTreeMap<String, Vec<PrefixEntry>>>,
    build: ObjectStoreSourceBuilder,
    default_cache: CachePolicy,
    process: ResolvedProcess,
}

impl ObjectStoreDiscovery {
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn from_config<T: TileSourceConfiguration>(
        config: &FileConfig<T>,
        extensions: &[&str],
        label: &'static str,
        reload_interval: Duration,
        id_resolver: IdResolver,
        default_cache: CachePolicy,
        process: &ProcessConfig,
        parser: ObjectStoreParser,
        build: ObjectStoreSourceBuilder,
    ) -> Self {
        let mut remote_prefixes = vec![];
        let collect = |path: &PathBuf| match SourceLocation::classify_path(path) {
            Ok(SourceLocation::ObjectStore(url) | SourceLocation::Http(url)) => {
                remote_prefixes.push(url);
            }
            Ok(SourceLocation::Local(_)) => {}
            Err(error) => tracing::warn!(
                "{label}: remote prefix {path:?} is not a valid URL ({error}); skipping"
            ),
        };
        config.paths.iter().for_each(collect);
        remote_prefixes.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        remote_prefixes.dedup();
        let configured = config
            .sources
            .values()
            .filter_map(|src| {
                SourceLocation::classify_path(src.get_path())
                    .ok()?
                    .into_url()
            })
            .map(|url| sanitized_url(&url))
            .collect();

        Self {
            remote_prefixes,
            configured,
            extensions: extensions
                .iter()
                .map(|extension| extension.to_ascii_lowercase())
                .collect(),
            label,
            id_resolver,
            reload_interval,
            parser,
            last_entries: Mutex::new(BTreeMap::new()),
            build,
            default_cache,
            process: process
                .resolve()
                .expect("the kind level carries no range-checked settings"),
        }
    }

    #[must_use]
    pub fn remote_prefixes(&self) -> &[Url] {
        &self.remote_prefixes
    }

    #[must_use]
    pub const fn reload_interval(&self) -> Duration {
        self.reload_interval
    }
}

impl Discovery for ObjectStoreDiscovery {
    type Args = Url;

    async fn discover(&self) -> SourceBuildResult<Discovered<Self::Args>> {
        let mut out: BTreeMap<String, (Version, Url)> = BTreeMap::new();
        for prefix in &self.remote_prefixes {
            let entries = match list_remote_prefix(
                prefix,
                &self.extensions,
                &self.configured,
                self.label,
                &self.id_resolver,
                &self.parser,
            )
            .await
            {
                Ok(entries) => {
                    self.last_entries
                        .lock()
                        .expect("prefix listing map mutex")
                        .insert(prefix.to_string(), entries.clone());
                    entries
                }
                Err(error) => {
                    let Some(entries) = self
                        .last_entries
                        .lock()
                        .expect("prefix listing map mutex")
                        .get(prefix.as_str())
                        .cloned()
                    else {
                        tracing::warn!(
                            "{}: list failed for {}: {error:?}; skipping prefix this tick",
                            self.label,
                            sanitized_url(prefix)
                        );
                        continue;
                    };
                    tracing::warn!(
                        "{}: list failed for {}: {error:?}; retaining last successful listing",
                        self.label,
                        sanitized_url(prefix)
                    );
                    entries
                }
            };
            for (id, url, version) in entries {
                out.insert(id, (version, url));
            }
        }
        Ok(Discovered::new(out))
    }

    async fn build(&self, id: &str, args: &Self::Args) -> SourceBuildResult<BuiltSource> {
        self.build
            .build(id.to_owned(), args.clone(), self.default_cache)
            .await
    }

    fn process(&self) -> ResolvedProcess {
        self.process.clone()
    }
}

fn version_from_meta(meta: &object_store::ObjectMeta) -> Version {
    if let Some(etag) = &meta.e_tag {
        Version::Tracked(xxhash_rust::xxh3::xxh3_128(etag.as_bytes()))
    } else {
        u128::try_from(meta.last_modified.timestamp_millis())
            .map_or(Version::Opaque, Version::Tracked)
    }
}

/// Lists the objects under `prefix` with one of `extensions`, skipping the `configured` ones
/// before they claim an id.
async fn list_remote_prefix(
    prefix: &Url,
    extensions: &[String],
    configured: &BTreeSet<String>,
    label: &str,
    id_resolver: &IdResolver,
    parser: &ObjectStoreParser,
) -> SourceBuildResult<Vec<PrefixEntry>> {
    let (store, base) = parser(prefix)
        .map_err(|error| ConfigFileError::ObjectStoreUrlParsing(error, sanitized_url(prefix)))?;
    let mut out = Vec::new();
    let mut stream = store.list(Some(&base));
    while let Some(meta) = stream
        .try_next()
        .await
        .map_err(|error| ConfigFileError::ObjectStoreList(error, sanitized_url(prefix)))?
    {
        let Some(filename) = meta.location.filename() else {
            continue;
        };
        let Some((stem, extension)) = filename.rsplit_once('.') else {
            continue;
        };
        if !extensions
            .iter()
            .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        {
            continue;
        }
        if stem.is_empty() {
            continue;
        }
        let mut object_url = prefix.clone();
        object_url.set_path(meta.location.as_ref());
        let sanitized = sanitized_url(&object_url);
        if configured.contains(&sanitized) {
            tracing::debug!(
                "{label}: {sanitized} is configured explicitly under `sources`; skipping its prefix discovery"
            );
            continue;
        }
        let id = id_resolver.resolve(stem, sanitized);
        out.push((id, object_url, version_from_meta(&meta)));
    }
    Ok(out)
}

fn sanitized_url(url: &Url) -> String {
    let mut result = format!("{}://", url.scheme());
    if let Some(host) = url.host_str() {
        result.push_str(host);
    }
    if let Some(port) = url.port() {
        result.push(':');
        result.push_str(&port.to_string());
    }
    result.push_str(url.path());
    result
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "pmtiles")]
    use std::path::PathBuf;
    #[cfg(feature = "pmtiles")]
    use std::sync::atomic::{AtomicBool, Ordering};

    use object_store::PutPayload;
    use object_store::memory::InMemory;

    use super::*;
    use crate::config::primitives::IdResolver;

    #[tokio::test]
    async fn prefix_discovery_filters_extensions_and_preserves_object_urls() {
        let store = InMemory::new();
        for path in [
            "imagery/vienna.tif",
            "imagery/ortho.TIFF",
            "imagery/.tif",
            "imagery/readme.txt",
            "outside/ignored.tif",
        ] {
            store
                .put(
                    &object_store::path::Path::from(path),
                    PutPayload::from_static(b"fixture"),
                )
                .await
                .unwrap();
        }
        let parser_store = store.clone();
        let parser: ObjectStoreParser = Box::new(move |_url: &Url| {
            Ok((
                Box::new(parser_store.clone()) as Box<dyn object_store::ObjectStore>,
                object_store::path::Path::from("imagery"),
            ))
        });
        let entries = list_remote_prefix(
            &Url::parse("https://user:secret@example.com:8443/imagery/?token=secret#fragment")
                .unwrap(),
            &["tif".to_owned(), "tiff".to_owned()],
            &BTreeSet::new(),
            "test",
            &IdResolver::new(&[]),
            &parser,
        )
        .await
        .unwrap();
        let found = entries
            .into_iter()
            .map(|(id, url, _)| (id, url.to_string()))
            .collect::<Vec<_>>();

        assert_eq!(
            found,
            [
                (
                    "ortho".to_owned(),
                    "https://user:secret@example.com:8443/imagery/ortho.TIFF?token=secret#fragment"
                        .to_owned(),
                ),
                (
                    "vienna".to_owned(),
                    "https://user:secret@example.com:8443/imagery/vienna.tif?token=secret#fragment"
                        .to_owned(),
                ),
            ]
        );
    }
    #[cfg(feature = "pmtiles")]
    #[tokio::test]
    async fn prefix_listing_failure_retains_the_last_successful_entries() {
        let store = InMemory::new();
        store
            .put(
                &object_store::path::Path::from("imagery/vienna.pmtiles"),
                PutPayload::from_static(b"fixture"),
            )
            .await
            .unwrap();
        let failing = Arc::new(AtomicBool::new(false));
        let failing_flag = Arc::clone(&failing);
        let parser_store = store.clone();
        let parser: ObjectStoreParser = Box::new(move |_url: &Url| {
            if failing_flag.load(Ordering::Relaxed) {
                return Err(object_store::Error::Generic {
                    store: "test",
                    source: Box::new(std::io::Error::other("boom")),
                });
            }
            Ok((
                Box::new(parser_store.clone()) as Box<dyn object_store::ObjectStore>,
                object_store::path::Path::from("imagery"),
            ))
        });
        let config: FileConfig<PmtConfig> =
            FileConfig::new(vec![PathBuf::from("s3://bucket/imagery/")]);
        let discovery = ObjectStoreDiscovery::from_config(
            &config,
            &["pmtiles"],
            "test",
            Duration::from_secs(1),
            IdResolver::new(&[]),
            CachePolicy::default(),
            &ProcessConfig::default(),
            parser,
            ObjectStoreSourceBuilder::Pmtiles(Box::default()),
        );

        let first = discovery.discover().await.unwrap().sources;
        assert_eq!(first.len(), 1);
        failing.store(true, Ordering::Relaxed);
        let retained = discovery.discover().await.unwrap().sources;

        assert_eq!(retained, first);
    }

    /// A `PMTiles` prefix discovery over `s3://bucket/` holding `objects`, listing `prefixes` and
    /// with `sources` configured as `(id, url)`.
    #[cfg(feature = "pmtiles")]
    async fn pmtiles_prefix_discovery(
        objects: &[&str],
        prefixes: &[&str],
        sources: &[(&str, &str)],
    ) -> ObjectStoreDiscovery {
        let store = InMemory::new();
        for object in objects {
            store
                .put(
                    &object_store::path::Path::from(*object),
                    PutPayload::from_static(b"fixture"),
                )
                .await
                .unwrap();
        }
        let parser: ObjectStoreParser = Box::new(move |url: &Url| {
            Ok((
                Box::new(store.clone()) as Box<dyn object_store::ObjectStore>,
                object_store::path::Path::from(url.path().trim_start_matches('/')),
            ))
        });
        let mut config: FileConfig<PmtConfig> =
            FileConfig::new(prefixes.iter().map(PathBuf::from).collect());
        for (id, url) in sources {
            config
                .sources
                .insert((*id).to_owned(), FileConfigSrc::Path(PathBuf::from(url)));
        }
        ObjectStoreDiscovery::from_config(
            &config,
            &["pmtiles"],
            "test",
            Duration::from_secs(1),
            IdResolver::new(&[]),
            CachePolicy::default(),
            &ProcessConfig::default(),
            parser,
            ObjectStoreSourceBuilder::Pmtiles(Box::default()),
        )
    }

    #[cfg(feature = "pmtiles")]
    #[tokio::test]
    async fn prefix_discovery_skips_objects_configured_under_sources() {
        let discovery = pmtiles_prefix_discovery(
            &["imagery/vienna.pmtiles", "imagery/graz.pmtiles"],
            &["s3://bucket/imagery/"],
            &[("vienna", "s3://bucket/imagery/vienna.pmtiles")],
        )
        .await;

        let discovered = discovery.discover().await.unwrap().sources;

        assert_eq!(discovered.keys().collect::<Vec<_>>(), ["graz"]);
    }

    #[cfg(feature = "pmtiles")]
    #[tokio::test]
    async fn a_skipped_object_does_not_claim_its_file_name_as_an_id() {
        // Prefixes are listed in sorted order, so the skipped `archive/` object comes up first.
        let discovery = pmtiles_prefix_discovery(
            &["archive/vienna.pmtiles", "imagery/vienna.pmtiles"],
            &["s3://bucket/imagery/", "s3://bucket/archive/"],
            &[("hillshade", "s3://bucket/archive/vienna.pmtiles")],
        )
        .await;

        let discovered = discovery.discover().await.unwrap().sources;

        let found = discovered
            .iter()
            .map(|(id, (_, url))| (id.as_str(), url.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(found, [("vienna", "s3://bucket/imagery/vienna.pmtiles")]);
    }
}

#[cfg(all(test, feature = "unstable-cog"))]
mod configured_object_tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use object_store::PutPayload;
    use object_store::memory::InMemory;
    use url::Url;

    use super::*;
    use crate::config::file::FileConfig;
    use crate::config::file::cog::CogConfig;

    fn cog_discovery(
        store: &InMemory,
        config: &FileConfig<CogConfig>,
        failing: bool,
    ) -> ConfiguredObjectDiscovery {
        let parser_store = store.clone();
        let parser: ObjectStoreParser = Box::new(move |url: &Url| {
            if failing {
                return Err(object_store::Error::Generic {
                    store: "test",
                    source: Box::new(std::io::Error::other("boom")),
                });
            }
            Ok((
                Box::new(parser_store.clone()) as Box<dyn object_store::ObjectStore>,
                object_store::path::Path::from(url.path().trim_start_matches('/')),
            ))
        });
        ConfiguredObjectDiscovery::from_config(
            FileKind::Cog,
            config,
            "test",
            Duration::from_secs(1),
            CachePolicy::default(),
            &ProcessConfig::default(),
            parser,
            ObjectStoreSourceBuilder::Cog(Box::default()),
        )
    }

    #[tokio::test]
    async fn configured_objects_are_discovered_and_versioned() {
        let store = InMemory::new();
        let path = object_store::path::Path::from("imagery/vienna.tif");
        store
            .put(&path, PutPayload::from_static(b"first"))
            .await
            .unwrap();
        let config = FileConfig {
            paths: Vec::new(),
            collections: Vec::new(),
            sources: BTreeMap::from([
                (
                    "remote".to_owned(),
                    FileConfigSrc::Path(PathBuf::from("s3://bucket/imagery/vienna.tif")),
                ),
                (
                    "local".to_owned(),
                    FileConfigSrc::Path(PathBuf::from("/tmp/elsewhere.tif")),
                ),
            ]),
            custom: CogConfig::default(),
        };
        let discovery = cog_discovery(&store, &config, false);

        assert_eq!(discovery.objects.len(), 1, "local sources are skipped");
        let (version, object) = &discovery.discover().await.unwrap().sources["remote"];
        assert!(matches!(version, Version::Tracked(_)));
        assert_eq!(object.url.as_str(), "s3://bucket/imagery/vienna.tif");

        store
            .put(&path, PutPayload::from_static(b"replaced"))
            .await
            .unwrap();
        let next = discovery.discover().await.unwrap().sources;
        assert_ne!(next["remote"].0, *version, "a replaced object re-versions");
    }

    #[tokio::test]
    async fn a_failed_check_retains_the_last_known_version() {
        let store = InMemory::new();
        let path = object_store::path::Path::from("imagery/vienna.tif");
        store
            .put(&path, PutPayload::from_static(b"first"))
            .await
            .unwrap();
        let config = FileConfig {
            paths: Vec::new(),
            collections: Vec::new(),
            sources: BTreeMap::from([(
                "remote".to_owned(),
                FileConfigSrc::Path(PathBuf::from("s3://bucket/imagery/vienna.tif")),
            )]),
            custom: CogConfig::default(),
        };

        let failing = Arc::new(AtomicBool::new(false));
        let failing_flag = Arc::clone(&failing);
        let parser_store = store.clone();
        let parser: ObjectStoreParser = Box::new(move |url: &Url| {
            if failing_flag.load(Ordering::Relaxed) {
                return Err(object_store::Error::Generic {
                    store: "test",
                    source: Box::new(std::io::Error::other("boom")),
                });
            }
            Ok((
                Box::new(parser_store.clone()) as Box<dyn object_store::ObjectStore>,
                object_store::path::Path::from(url.path().trim_start_matches('/')),
            ))
        });
        let discovery = ConfiguredObjectDiscovery::from_config(
            FileKind::Cog,
            &config,
            "test",
            Duration::from_secs(1),
            CachePolicy::default(),
            &ProcessConfig::default(),
            parser,
            ObjectStoreSourceBuilder::Cog(Box::default()),
        );

        let first = discovery.discover().await.unwrap().sources;
        store.delete(&path).await.unwrap();
        failing.store(true, Ordering::Relaxed);
        let retained = discovery.discover().await.unwrap().sources;

        assert_eq!(retained["remote"].0, first["remote"].0);
    }
}
