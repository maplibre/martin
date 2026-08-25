//! [`FsDiscovery`]: a [`Discovery`] over local directories, used by the file-backed kinds.
//! Each kind differs only by its extension list and a build closure.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use futures::future::BoxFuture;
use martin_core::tiles::BoxedSource;
use tokio::fs::{self, DirEntry};

use crate::config::file::FileConfigSrc;
use crate::config::file::file_config::is_remote_url;
use crate::config::file::tiles::discovery::{BuiltSource, Discovered, Discovery, Version};
use crate::config::file::{
    CachePolicy, FileConfigEnum, ProcessConfig, SourceBuildError, SourceBuildResult,
};
use crate::config::primitives::{IdResolver, OptOneMany};

/// The future an [`FsSourceBuilder`] returns: the freshly-built source, or an init error.
type BuildFuture = BoxFuture<'static, SourceBuildResult<BoxedSource>>;

/// Opens one discovered file as a source.
///
/// This is a boxed `dyn Fn`, not a bare `fn` pointer.
/// The `PMTiles` builder must capture per-source state.
/// It closes over the shared directory cache and the configured `object_store` options so every discovered file reuses them.
/// A captured closure has an unnameable type.
/// Storing it in [`FsDiscovery`]'s `build` field therefore requires erasing it behind a `Box<dyn Fn>`.
/// The mbtiles/cog builders capture nothing and would coerce to a bare `fn` pointer.
/// They share this one type so all kinds yield the same concrete `FsDiscovery`.
/// The cost is a single heap allocation per reloader at startup.
pub type FsSourceBuilder = Box<dyn Fn(String, PathBuf, CachePolicy) -> BuildFuture + Send + Sync>;

/// What the config says about one explicitly-configured source, so a discovered file with the
/// same canonical path keeps its policy and per-source process settings.
struct ConfiguredSource {
    policy: CachePolicy,
    /// Per-source `convert_to_*` / `cache_control` override, already resolved against the kind level.
    process: Option<ProcessConfig>,
}

/// A [`Discovery`] that enumerates source files under the watched directories.
pub struct FsDiscovery {
    directories: Vec<PathBuf>,
    extensions: &'static [&'static str],
    /// Canonical path -> configured entry for explicitly-configured sources.
    configured: BTreeMap<PathBuf, ConfiguredSource>,
    id_resolver: IdResolver,
    process: ProcessConfig,
    build: FsSourceBuilder,
}

impl FsDiscovery {
    /// Collects the local watch directories and per-path cache policies; remote URLs are skipped.
    pub fn from_config<C>(
        config: &FileConfigEnum<C>,
        extensions: &'static [&'static str],
        id_resolver: IdResolver,
        process: ProcessConfig,
        build: FsSourceBuilder,
    ) -> Self {
        let mut directories: Vec<PathBuf> = vec![];
        let mut configured: BTreeMap<PathBuf, ConfiguredSource> = BTreeMap::new();

        if let FileConfigEnum::Config(cfg) = config
            && let Some(sources) = &cfg.sources
        {
            for (id, src) in sources {
                let path = src.get_path();
                if is_remote_url(path) {
                    continue;
                }
                let Ok(canonical) = path.canonicalize() else {
                    tracing::warn!(source.id = %id, path = ?path, "failed to canonicalize tile source path");
                    continue;
                };
                configured.insert(
                    canonical,
                    ConfiguredSource {
                        policy: src.cache_zoom(),
                        process: per_source_process(&process, src),
                    },
                );
            }
        }

        let mut push_local = |path: &PathBuf| {
            if is_remote_url(path) {
                return;
            }
            match path.canonicalize() {
                Ok(p) => directories.push(p),
                Err(e) => {
                    tracing::warn!(directory = ?path, error = %e, "failed to canonicalize watch directory");
                }
            }
        };

        match config {
            FileConfigEnum::Config(cfg) => match &cfg.paths {
                OptOneMany::One(path) => push_local(path),
                OptOneMany::Many(paths) => paths.iter().for_each(&mut push_local),
                OptOneMany::NoVals => {}
            },
            FileConfigEnum::Path(path) => push_local(path),
            FileConfigEnum::Paths(paths) => paths.iter().for_each(push_local),
            FileConfigEnum::None => {}
        }

        directories.sort();
        directories.dedup();

        Self {
            directories,
            extensions,
            configured,
            id_resolver,
            process,
            build,
        }
    }

    /// The watched directories, for wiring a `NotifyTrigger`.
    #[must_use]
    pub fn directories(&self) -> &[PathBuf] {
        &self.directories
    }
}

/// Per-source `convert_to_*` and `cache_control` settings override the kind-level [`ProcessConfig`].
fn per_source_process(kind_level: &ProcessConfig, src: &FileConfigSrc) -> Option<ProcessConfig> {
    use crate::config::file::resolve_process_config;

    let FileConfigSrc::Obj(obj) = src else {
        return None;
    };
    let per_source = ProcessConfig {
        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        convert_to_mlt: obj.convert_to_mlt.clone(),
        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        convert_to_mvt: obj.convert_to_mvt.clone(),
        cache_control: obj.cache_control.clone(),
    };
    if per_source == ProcessConfig::default() {
        return None;
    }
    Some(resolve_process_config(
        kind_level,
        &ProcessConfig::default(),
        &per_source,
    ))
}

impl Discovery for FsDiscovery {
    type Args = (PathBuf, CachePolicy);

    async fn discover(&self) -> SourceBuildResult<Discovered<Self::Args>> {
        let discovered = discover_sources_by_ext(
            &self.directories,
            self.extensions,
            &self.configured,
            &self.id_resolver,
        )
        .await?;

        Ok(Discovered::new(
            discovered
                .into_iter()
                .map(|(id, (path, modified_at_ms, policy))| {
                    (id, (Version::Tracked(modified_at_ms), (path, policy)))
                })
                .collect(),
        ))
    }

    async fn build(&self, id: &str, args: &Self::Args) -> SourceBuildResult<BuiltSource> {
        let source = (self.build)(id.to_owned(), args.0.clone(), args.1).await?;
        let process = self
            .configured
            .get(&args.0)
            .and_then(|cfg| cfg.process.clone());
        Ok(BuiltSource { source, process })
    }

    fn process(&self) -> ProcessConfig {
        self.process.clone()
    }
}

struct ResolvedEntry {
    path: PathBuf,
    stem: String,
    path_str: String,
    modified_ms: u128,
}

fn path_modified_ms(path: &Path) -> Option<u128> {
    let Ok(metadata) = path.metadata() else {
        tracing::warn!(path = ?path, "failed to resolve metadata");
        return None;
    };

    let Ok(modified) = metadata.modified() else {
        tracing::warn!(path = ?path, "failed to resolve modified timestamp");
        return None;
    };

    let Ok(duration) = modified.duration_since(UNIX_EPOCH) else {
        tracing::warn!(path = ?path, "failed to resolve duration since unix epoch");
        return None;
    };

    Some(duration.as_millis())
}

fn resolve_dir_entry(entry: &DirEntry) -> Option<ResolvedEntry> {
    let raw = entry.path();

    let Ok(path) = raw.canonicalize() else {
        tracing::warn!(path = ?raw, "failed to canonicalize path");
        return None;
    };

    let Some(stem) = path.file_stem().and_then(|o| o.to_str()) else {
        tracing::warn!(path = ?path, "failed to resolve file stem");
        return None;
    };

    let Ok(path_str) = path.clone().into_os_string().into_string() else {
        tracing::warn!(path = ?path, "failed to resolve path string");
        return None;
    };

    let modified_ms = path_modified_ms(&path)?;

    Some(ResolvedEntry {
        path: path.clone(),
        stem: stem.to_owned(),
        path_str,
        modified_ms,
    })
}

/// Scans `directories` for files matching `extensions`, resolving ids and cache policies.
async fn discover_sources_by_ext(
    directories: &[PathBuf],
    extensions: &[&str],
    configured: &BTreeMap<PathBuf, ConfiguredSource>,
    id_resolver: &IdResolver,
) -> SourceBuildResult<BTreeMap<String, (PathBuf, u128, CachePolicy)>> {
    let mut out = BTreeMap::new();
    for directory in directories {
        let mut entries = fs::read_dir(directory)
            .await
            .map_err(SourceBuildError::Io)?;
        while let Some(entry) = entries.next_entry().await.map_err(SourceBuildError::Io)? {
            let Some(e) = resolve_dir_entry(&entry) else {
                continue;
            };
            if !e.path.is_file()
                || e.path
                    .extension()
                    .is_none_or(|ext| !extensions.iter().any(|ex| *ex == ext))
            {
                continue;
            }
            let policy = configured
                .get(&e.path)
                .map(|cfg| cfg.policy)
                .unwrap_or_default();
            let id = id_resolver.resolve(&e.stem, e.path_str.clone());
            out.insert(id, (e.path, e.modified_ms, policy));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    fn unreachable_builder() -> FsSourceBuilder {
        Box::new(|id, _path, _policy| {
            Box::pin(async move { panic!("build should not be called by discover(): {id}") })
        })
    }

    #[tokio::test]
    async fn discover_finds_matching_files_with_tracked_versions() {
        let dir = tempfile::tempdir().expect("tempdir");
        File::create(dir.path().join("alpha.mbtiles")).expect("create alpha");
        File::create(dir.path().join("beta.mbtiles")).expect("create beta");
        File::create(dir.path().join("ignore.txt")).expect("create ignore");

        let discovery = FsDiscovery::from_config(
            &FileConfigEnum::<()>::Path(dir.path().to_path_buf()),
            &["mbtiles"],
            IdResolver::new(&[]),
            ProcessConfig::default(),
            unreachable_builder(),
        );

        let snapshot = discovery.discover().await.expect("discover").sources;

        let mut ids: Vec<&String> = snapshot.keys().collect();
        ids.sort();
        assert_eq!(ids, vec!["alpha", "beta"]);
        assert!(
            snapshot
                .values()
                .all(|(v, _)| matches!(v, Version::Tracked(_))),
            "file sources carry a Tracked mtime version"
        );
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn configured_sources_keep_their_convert_override() {
        use crate::config::file::{FileConfig, FileConfigSource};
        use crate::config::primitives::AutoOption;

        let dir = tempfile::tempdir().expect("tempdir");
        let overridden = dir.path().join("overridden.mbtiles");
        let plain = dir.path().join("plain.mbtiles");
        File::create(&overridden).expect("create overridden");
        File::create(&plain).expect("create plain");

        let config = FileConfigEnum::Config(FileConfig {
            sources: Some(BTreeMap::from([
                (
                    "overridden".to_owned(),
                    FileConfigSrc::Obj(FileConfigSource {
                        path: overridden.clone(),
                        convert_to_mlt: Some(AutoOption::Disabled),
                        convert_to_mvt: None,
                        cache_control: None,
                        cache: CachePolicy::default(),
                    }),
                ),
                ("plain".to_owned(), FileConfigSrc::Path(plain.clone())),
            ])),
            ..FileConfig::<()>::default()
        });
        let kind_level = ProcessConfig {
            convert_to_mlt: Some(AutoOption::Auto),
            convert_to_mvt: None,
            cache_control: None,
        };
        let discovery = FsDiscovery::from_config(
            &config,
            &["mbtiles"],
            IdResolver::new(&[]),
            kind_level,
            unreachable_builder(),
        );

        let configured = |path: &PathBuf| {
            let canonical = path.canonicalize().expect("canonicalize");
            discovery
                .configured
                .get(&canonical)
                .expect("configured path")
        };
        assert_eq!(
            configured(&overridden)
                .process
                .as_ref()
                .and_then(|p| p.convert_to_mlt.clone()),
            Some(AutoOption::Disabled)
        );
        assert_eq!(configured(&plain).process, None);
    }

    #[test]
    fn configured_sources_keep_their_cache_control_override() {
        use crate::config::file::{FileConfig, FileConfigSource};

        let dir = tempfile::tempdir().expect("tempdir");
        let pinned = dir.path().join("pinned.mbtiles");
        File::create(&pinned).expect("create pinned");

        let config = FileConfigEnum::Config(FileConfig {
            sources: Some(BTreeMap::from([(
                "pinned".to_owned(),
                FileConfigSrc::Obj(FileConfigSource {
                    path: pinned.clone(),
                    #[cfg(all(feature = "mlt", feature = "_tiles"))]
                    convert_to_mlt: None,
                    #[cfg(all(feature = "mlt", feature = "_tiles"))]
                    convert_to_mvt: None,
                    cache_control: Some(
                        serde_saphyr::from_str("public, max-age=60").expect("valid header"),
                    ),
                    cache: CachePolicy::default(),
                }),
            )])),
            ..FileConfig::<()>::default()
        });
        let discovery = FsDiscovery::from_config(
            &config,
            &["mbtiles"],
            IdResolver::new(&[]),
            ProcessConfig::default(),
            unreachable_builder(),
        );

        let canonical = pinned.canonicalize().expect("canonicalize");
        let process = discovery
            .configured
            .get(&canonical)
            .expect("configured path")
            .process
            .as_ref()
            .expect("a cache_control-only override is still an override");
        assert!(process.cache_control.is_some());
    }
}
