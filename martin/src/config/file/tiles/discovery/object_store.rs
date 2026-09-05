//! Storage-neutral discovery over remote object prefixes.

use std::collections::BTreeMap;
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
    CachePolicy, ConfigFileError, FileConfigEnum, FileConfigSrc, SourceBuildResult,
    TileSourceConfiguration,
};
use crate::config::primitives::{IdResolver, OptOneMany};
use crate::reload::{FileKind, SourceProvenance};
#[cfg(feature = "unstable-cog")]
use martin_core::tiles::cog::CogObjectMeta;

pub type ObjectStoreParser = Box<
    dyn Fn(
            &Url,
        )
            -> object_store::Result<(Box<dyn object_store::ObjectStore>, object_store::path::Path)>
        + Send
        + Sync,
>;

/// Builds a source discovered in an object store.
///
/// The enum keeps the supported source kinds explicit and avoids erasing async builders behind
/// boxed, pinned futures. Future remote-backed source kinds can add a variant here.
pub enum ObjectStoreSourceBuilder {
    #[cfg(feature = "pmtiles")]
    Pmtiles(PmtConfig),
    #[cfg(feature = "unstable-cog")]
    Cog(CogConfig),
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
            Self::Pmtiles(config) => config.new_sources_url(id, url, cache).await.map(Into::into),
            #[cfg(feature = "unstable-cog")]
            Self::Cog(config) => config.new_sources_url(id, url, cache).await.map(Into::into),
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

/// A [`Discovery`] over explicitly configured remote objects: the `sources` map entries and
/// `paths` entries that name one remote object.
///
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
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn from_config<T: TileSourceConfiguration>(
        kind: FileKind,
        config: &FileConfigEnum<T>,
        extensions: &[&str],
        label: &'static str,
        reload_interval: Duration,
        id_resolver: &IdResolver,
        default_cache: CachePolicy,
        process: &ProcessConfig,
        parser: ObjectStoreParser,
        build: ObjectStoreSourceBuilder,
        initial_versions: BTreeMap<String, Version>,
    ) -> Self {
        let mut objects: BTreeMap<String, ConfiguredObject> = BTreeMap::new();
        if let FileConfigEnum::Config(cfg) = config
            && let Some(sources) = &cfg.sources
        {
            for (id, src) in sources {
                let Ok(SourceLocation::ObjectStore(url) | SourceLocation::Http(url)) =
                    SourceLocation::classify_path(src.get_path())
                else {
                    // Local sources belong to the file-based discovery.
                    continue;
                };
                let resolved = id_resolver.resolve(id, sanitized_url(&url));
                objects.insert(
                    resolved.clone(),
                    ConfiguredObject {
                        id: resolved,
                        url,
                        policy: src.cache_zoom().or(default_cache),
                        process: per_source_process(process, src),
                        src: src.clone(),
                    },
                );
            }
        }
        // A `paths` entry that is a remote URL naming one object (its path ends with an allowed
        // extension) is polled like a configured source; prefix URLs are another feature.
        let mut collect = |path: &PathBuf| {
            let Ok(SourceLocation::ObjectStore(url) | SourceLocation::Http(url)) =
                SourceLocation::classify_path(path)
            else {
                return;
            };
            let Some(filename) = url.path().rsplit('/').next() else {
                return;
            };
            let Some((stem, extension)) = filename.rsplit_once('.') else {
                return;
            };
            if stem.is_empty()
                || !extensions
                    .iter()
                    .any(|allowed| extension.eq_ignore_ascii_case(allowed))
            {
                return;
            }
            let resolved = id_resolver.resolve(stem, sanitized_url(&url));
            objects.insert(
                resolved.clone(),
                ConfiguredObject {
                    id: resolved,
                    url,
                    policy: default_cache,
                    process: None,
                    src: FileConfigSrc::Path(path.clone()),
                },
            );
        };
        match config {
            FileConfigEnum::Config(cfg) => match &cfg.paths {
                OptOneMany::One(path) => collect(path),
                OptOneMany::Many(paths) => paths.iter().for_each(&mut collect),
                OptOneMany::NoVals => {}
            },
            FileConfigEnum::Path(path) => collect(path),
            FileConfigEnum::Paths(paths) => paths.iter().for_each(collect),
            FileConfigEnum::None => {}
        }

        Self {
            kind,
            label,
            objects: objects.into_values().collect(),
            reload_interval,
            last_versions: Mutex::new(initial_versions),
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

    /// Baseline matching the remote COG sources that startup successfully opened.
    #[must_use]
    pub fn loaded_baseline(&self) -> BTreeMap<String, (Version, ConfiguredObject)> {
        let versions = self.last_versions.lock().expect("version map mutex");
        self.objects
            .iter()
            .filter_map(|object| {
                versions
                    .get(&object.id)
                    .copied()
                    .map(|version| (object.id.clone(), (version, object.clone())))
            })
            .collect()
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
        let process = args
            .process
            .as_ref()
            .map(|pc| pc.resolve().map_err(|e| e.for_source(id.to_owned())))
            .transpose()?;
        Ok(BuiltSource {
            source,
            process,
            provenance: Some(SourceProvenance::File {
                kind: self.kind,
                src: args.src.clone(),
            }),
        })
    }

    fn process(&self) -> ResolvedProcess {
        self.process.clone()
    }
}

/// A [`Discovery`] over one or more remote object-store prefixes.
pub struct ObjectStoreDiscovery {
    remote_prefixes: Vec<Url>,
    extensions: Arc<[String]>,
    label: &'static str,
    id_resolver: IdResolver,
    reload_interval: Duration,
    parser: ObjectStoreParser,
    build: ObjectStoreSourceBuilder,
    default_cache: CachePolicy,
    process: ResolvedProcess,
}

impl ObjectStoreDiscovery {
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn from_config<T: TileSourceConfiguration>(
        config: &FileConfigEnum<T>,
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
        let mut collect = |path: &PathBuf| match SourceLocation::classify_path(path) {
            Ok(SourceLocation::ObjectStore(url) | SourceLocation::Http(url)) => {
                remote_prefixes.push(url);
            }
            Ok(SourceLocation::Local(_)) => {}
            Err(error) => tracing::warn!(
                "{label}: remote prefix {path:?} is not a valid URL ({error}); skipping"
            ),
        };
        match config {
            FileConfigEnum::Config(cfg) => match &cfg.paths {
                OptOneMany::One(path) => collect(path),
                OptOneMany::Many(paths) => paths.iter().for_each(&mut collect),
                OptOneMany::NoVals => {}
            },
            FileConfigEnum::Path(path) => collect(path),
            FileConfigEnum::Paths(paths) => paths.iter().for_each(collect),
            FileConfigEnum::None => {}
        }
        remote_prefixes.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        remote_prefixes.dedup();

        Self {
            remote_prefixes,
            extensions: extensions
                .iter()
                .map(|extension| extension.to_ascii_lowercase())
                .collect(),
            label,
            id_resolver,
            reload_interval,
            parser,
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
            match list_remote_prefix(prefix, &self.extensions, &self.id_resolver, &self.parser)
                .await
            {
                Ok(entries) => {
                    for (id, url, version) in entries {
                        out.insert(id, (version, url));
                    }
                }
                Err(error) => tracing::warn!(
                    "{}: list failed for {}: {error:?}; skipping prefix this tick",
                    self.label,
                    sanitized_url(prefix)
                ),
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

fn version_from_parts(e_tag: Option<&str>, last_modified_millis: Option<i64>) -> Version {
    if let Some(etag) = e_tag {
        Version::Tracked(xxhash_rust::xxh3::xxh3_128(etag.as_bytes()))
    } else {
        last_modified_millis
            .and_then(|timestamp| u128::try_from(timestamp).ok())
            .map_or(Version::Opaque, Version::Tracked)
    }
}

fn version_from_meta(meta: &object_store::ObjectMeta) -> Version {
    version_from_parts(
        meta.e_tag.as_deref(),
        Some(meta.last_modified.timestamp_millis()),
    )
}

#[cfg(feature = "unstable-cog")]
pub(crate) fn version_from_cog_meta(meta: &CogObjectMeta) -> Version {
    version_from_parts(meta.e_tag.as_deref(), meta.last_modified_millis)
}

async fn list_remote_prefix(
    prefix: &Url,
    extensions: &[String],
    id_resolver: &IdResolver,
    parser: &ObjectStoreParser,
) -> SourceBuildResult<Vec<(String, Url, Version)>> {
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
        let object_url_str = format!(
            "{}://{}/{}",
            prefix.scheme(),
            prefix.host_str().unwrap_or(""),
            meta.location
        );
        let Ok(object_url) = Url::parse(&object_url_str) else {
            tracing::warn!("cannot build absolute URL from {object_url_str}");
            continue;
        };
        let id = id_resolver.resolve(stem, object_url.to_string());
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
            &Url::parse("s3://bucket/imagery/").unwrap(),
            &["tif".to_owned(), "tiff".to_owned()],
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
                    "s3://bucket/imagery/ortho.TIFF".to_owned()
                ),
                (
                    "vienna".to_owned(),
                    "s3://bucket/imagery/vienna.tif".to_owned()
                ),
            ]
        );
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
    use crate::config::file::cog::CogConfig;
    use crate::config::file::{FileConfig, FileConfigEnum};
    use crate::config::primitives::IdResolver;

    fn cog_discovery(
        store: &InMemory,
        config: &FileConfigEnum<CogConfig>,
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
            &["tif", "tiff"],
            "test",
            Duration::from_secs(1),
            &IdResolver::new(&[]),
            CachePolicy::default(),
            &ProcessConfig::default(),
            parser,
            ObjectStoreSourceBuilder::Cog(CogConfig::default()),
            BTreeMap::new(),
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
        let config = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::NoVals,
            collections: OptOneMany::NoVals,
            sources: Some(BTreeMap::from([
                (
                    "remote".to_owned(),
                    FileConfigSrc::Path(PathBuf::from("s3://bucket/imagery/vienna.tif")),
                ),
                (
                    "local".to_owned(),
                    FileConfigSrc::Path(PathBuf::from("/tmp/elsewhere.tif")),
                ),
            ])),
            custom: CogConfig::default(),
        });
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
    async fn a_paths_entry_naming_one_object_is_polled_but_a_prefix_is_not() {
        let store = InMemory::new();
        store
            .put(
                &object_store::path::Path::from("imagery/ortho.tif"),
                PutPayload::from_static(b"fixture"),
            )
            .await
            .unwrap();
        let config = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::Many(vec![
                PathBuf::from("https://host/imagery/ortho.tif"),
                PathBuf::from("https://host/imagery/prefix/"),
            ]),
            collections: OptOneMany::NoVals,
            sources: None,
            custom: CogConfig::default(),
        });
        let discovery = cog_discovery(&store, &config, false);

        assert_eq!(discovery.objects.len(), 1);
        assert_eq!(discovery.objects[0].id, "ortho");
    }

    #[tokio::test]
    async fn a_failed_check_retains_the_last_known_version() {
        let store = InMemory::new();
        let path = object_store::path::Path::from("imagery/vienna.tif");
        store
            .put(&path, PutPayload::from_static(b"first"))
            .await
            .unwrap();
        let config = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::NoVals,
            collections: OptOneMany::NoVals,
            sources: Some(BTreeMap::from([(
                "remote".to_owned(),
                FileConfigSrc::Path(PathBuf::from("s3://bucket/imagery/vienna.tif")),
            )])),
            custom: CogConfig::default(),
        });

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
            &["tif", "tiff"],
            "test",
            Duration::from_secs(1),
            &IdResolver::new(&[]),
            CachePolicy::default(),
            &ProcessConfig::default(),
            parser,
            ObjectStoreSourceBuilder::Cog(CogConfig::default()),
            BTreeMap::new(),
        );

        let first = discovery.discover().await.unwrap().sources;
        store.delete(&path).await.unwrap();
        failing.store(true, Ordering::Relaxed);
        let retained = discovery.discover().await.unwrap().sources;

        assert_eq!(retained["remote"].0, first["remote"].0);
    }
}
