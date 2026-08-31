//! [`FsDiscovery`]: a [`Discovery`] over local directories, used by the file-backed kinds.
//! Each kind differs only by its extension list and a build closure.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use futures::future::BoxFuture;
use martin_core::tiles::BoxedSource;
use tokio::fs::{self, DirEntry};

use crate::config::file::file_config::is_remote_url;
use crate::config::file::tiles::discovery::{BuiltSource, Discovered, Discovery, Version};
use crate::config::file::{
    CachePolicy, FileConfigEnum, FileConfigSrc, ProcessConfig, ResolvedProcess, SourceBuildError,
    SourceBuildResult, TileSourceWarning,
};
use crate::config::primitives::{IdResolver, OptOneMany};
use crate::reload::{FileKind, SourceProvenance};

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
    /// Per-source `convert_to_*` / `cache_control` override layered over the kind level.
    /// Resolved when the source is built so a bad parameter fails that source alone.
    process: Option<ProcessConfig>,
    /// The entry as configured, preserved verbatim for `--save-config`.
    src: FileConfigSrc,
}

/// A [`Discovery`] that enumerates source files under the watched directories.
pub struct FsDiscovery {
    kind: FileKind,
    /// The watched directories as configured.
    directories: Vec<PathBuf>,
    extensions: &'static [&'static str],
    /// Canonical path -> configured entry for explicitly-configured sources.
    configured: BTreeMap<PathBuf, ConfiguredSource>,
    id_resolver: IdResolver,
    /// The kind level, resolved once for every source without its own override.
    process: ResolvedProcess,
    build: FsSourceBuilder,
    /// Configured directories that could not be read at construction.
    warnings: Vec<TileSourceWarning>,
}

impl FsDiscovery {
    /// Collects the local watch directories and per-path config entries.
    /// Remote URLs are skipped.
    pub fn from_config<C>(
        kind: FileKind,
        config: &FileConfigEnum<C>,
        extensions: &'static [&'static str],
        id_resolver: IdResolver,
        process: &ProcessConfig,
        build: FsSourceBuilder,
    ) -> Self {
        let mut directories: Vec<PathBuf> = vec![];
        let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
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
                        process: per_source_process(process, src),
                        src: src.clone(),
                    },
                );
            }
        }

        let mut warnings: Vec<TileSourceWarning> = vec![];
        let mut push_local = |path: &PathBuf| {
            if is_remote_url(path) {
                return;
            }
            let probed = path
                .canonicalize()
                .and_then(|p| std::fs::read_dir(&p).map(|_| p));
            match probed {
                Ok(canonical) => {
                    if seen.insert(canonical) {
                        directories.push(path.clone());
                    }
                }
                Err(e) => {
                    tracing::warn!(directory = ?path, error = %e, "cannot read watch directory");
                    warnings.push(TileSourceWarning::PathError {
                        path: path.clone(),
                        error: e.to_string(),
                    });
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

        Self {
            kind,
            directories,
            extensions,
            configured,
            id_resolver,
            process: process
                .resolve()
                .expect("the kind level carries no range-checked settings"),
            build,
            warnings,
        }
    }

    /// The canonical watched directories, for wiring a `NotifyTrigger`.
    #[must_use]
    pub fn directories(&self) -> Vec<PathBuf> {
        self.directories
            .iter()
            .filter_map(|dir| dir.canonicalize().ok())
            .collect()
    }

    /// The config-file entry for a discovered file.
    /// A configured source keeps its entry and a discovered file is spelled through its configured directory.
    fn config_entry(&self, canonical: &Path) -> FileConfigSrc {
        if let Some(cfg) = self.configured.get(canonical) {
            return cfg.src.clone();
        }
        let as_configured = self
            .directories
            .iter()
            .filter_map(|dir| {
                let canonical_dir = dir.canonicalize().ok()?;
                let relative = canonical.strip_prefix(&canonical_dir).ok()?;
                Some((canonical_dir.components().count(), dir.join(relative)))
            })
            .max_by_key(|(depth, _)| *depth)
            .map(|(_, path)| path);
        FileConfigSrc::Path(as_configured.unwrap_or_else(|| canonical.to_path_buf()))
    }
}

/// Per-source `convert_to_*` and `cache_control` settings layered over the kind-level [`ProcessConfig`].
fn per_source_process(kind_level: &ProcessConfig, src: &FileConfigSrc) -> Option<ProcessConfig> {
    let FileConfigSrc::Obj(obj) = src else {
        return None;
    };
    let per_source = ProcessConfig {
        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        convert_to_mlt: obj.convert_to_mlt.clone(),
        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        convert_to_mvt: obj.convert_to_mvt.clone(),
        cache_control: obj.cache_control.clone(),
        #[cfg(feature = "hillshade")]
        convert_to_hillshade: obj.convert_to_hillshade.clone(),
        #[cfg(all(feature = "contour", feature = "_tiles"))]
        convert_to_contour: obj.convert_to_contour.clone(),
    };
    if per_source == ProcessConfig::default() {
        return None;
    }
    Some(ProcessConfig::layered(
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
            .and_then(|cfg| cfg.process.as_ref())
            .map(|pc| pc.resolve().map_err(|e| e.for_source(id.to_owned())))
            .transpose()?;
        Ok(BuiltSource {
            source,
            process,
            provenance: Some(SourceProvenance::File {
                kind: self.kind,
                src: self.config_entry(&args.0),
            }),
        })
    }

    fn process(&self) -> ResolvedProcess {
        self.process.clone()
    }

    fn construction_warnings(&self) -> Vec<TileSourceWarning> {
        self.warnings.clone()
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
#[cfg(feature = "mbtiles")]
mod tests {
    use std::fs::File;

    use async_trait::async_trait;
    use insta::assert_yaml_snapshot;
    use martin_core::CacheZoomRange;
    use martin_core::tiles::{MartinCoreResult, Source, UrlQuery};
    use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
    use tilejson::{TileJSON, tilejson};

    use super::*;
    use crate::TileSourceManager;
    use crate::config::file::tiles::driver::ReloadDriver;
    use crate::config::file::{ConfigFileError, OnInvalid};

    /// Files whose stem starts with this prefix fail to build.
    const BAD_PREFIX: &str = "bad_";

    #[derive(Debug, Clone)]
    struct TestSource {
        id: String,
        tj: TileJSON,
    }

    #[async_trait]
    impl Source for TestSource {
        fn get_id(&self) -> &str {
            &self.id
        }
        fn get_tilejson(&self) -> &TileJSON {
            &self.tj
        }
        fn get_tile_info(&self) -> TileInfo {
            TileInfo::new(Format::Mvt, Encoding::Uncompressed)
        }
        fn clone_source(&self) -> BoxedSource {
            Box::new(self.clone())
        }
        fn cache_zoom(&self) -> CacheZoomRange {
            CacheZoomRange::default()
        }
        async fn get_tile(
            &self,
            _xyz: TileCoord,
            _url_query: Option<&UrlQuery>,
        ) -> MartinCoreResult<TileData> {
            Ok(vec![])
        }
    }

    /// Opens every file as a [`TestSource`] except the `bad_` ones, which fail to build.
    fn fake_builder() -> FsSourceBuilder {
        Box::new(|id, path, _policy| {
            Box::pin(async move {
                if id.starts_with(BAD_PREFIX) {
                    return Err(SourceBuildError::from(ConfigFileError::InvalidFilePath(
                        path,
                    )));
                }
                Ok(Box::new(TestSource {
                    id,
                    tj: tilejson! { tiles: vec![] },
                }) as BoxedSource)
            })
        })
    }

    fn unreachable_builder() -> FsSourceBuilder {
        Box::new(|id, _path, _policy| {
            Box::pin(async move { panic!("build should not be called by discover(): {id}") })
        })
    }

    fn sorted_source_names(catalog: &TileSourceManager) -> Vec<String> {
        let mut names = catalog.tile_sources().source_names();
        names.sort();
        names
    }

    #[tokio::test]
    async fn discover_finds_matching_files_with_tracked_versions() {
        let dir = tempfile::tempdir().expect("tempdir");
        File::create(dir.path().join("alpha.mbtiles")).expect("create alpha");
        File::create(dir.path().join("beta.mbtiles")).expect("create beta");
        File::create(dir.path().join("ignore.txt")).expect("create ignore");

        let discovery = FsDiscovery::from_config(
            FileKind::Mbtiles,
            &FileConfigEnum::<()>::Path(dir.path().to_path_buf()),
            &["mbtiles"],
            IdResolver::new(&[]),
            &ProcessConfig::default(),
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

    #[tokio::test]
    async fn init_publishes_the_good_files_and_skips_the_bad_one_under_warn() {
        let dir = tempfile::tempdir().expect("tempdir");
        File::create(dir.path().join("good_0.mbtiles")).expect("create good_0");
        File::create(dir.path().join("good_1.mbtiles")).expect("create good_1");
        File::create(dir.path().join("bad_0.mbtiles")).expect("create bad_0");

        let discovery = FsDiscovery::from_config(
            FileKind::Mbtiles,
            &FileConfigEnum::<()>::Path(dir.path().to_path_buf()),
            &["mbtiles"],
            IdResolver::new(&[]),
            &ProcessConfig::default(),
            fake_builder(),
        );
        let catalog = TileSourceManager::new(None, OnInvalid::Warn);
        ReloadDriver::new(discovery, catalog.clone())
            .init()
            .await
            .expect("the warn policy skips the bad file");

        assert_yaml_snapshot!(sorted_source_names(&catalog), @"
        - good_0
        - good_1
        ");
    }

    #[tokio::test]
    async fn init_fails_on_the_bad_file_under_abort() {
        let dir = tempfile::tempdir().expect("tempdir");
        File::create(dir.path().join("good_0.mbtiles")).expect("create good_0");
        File::create(dir.path().join("bad_0.mbtiles")).expect("create bad_0");

        let discovery = FsDiscovery::from_config(
            FileKind::Mbtiles,
            &FileConfigEnum::<()>::Path(dir.path().to_path_buf()),
            &["mbtiles"],
            IdResolver::new(&[]),
            &ProcessConfig::default(),
            fake_builder(),
        );
        let catalog = TileSourceManager::new(None, OnInvalid::Abort);
        let error = ReloadDriver::new(discovery, catalog)
            .init()
            .await
            .expect_err("the abort policy fails init");

        let prefix = dir
            .path()
            .canonicalize()
            .expect("canonicalize")
            .to_string_lossy()
            .to_string();
        assert_yaml_snapshot!(error.to_string().replace(&prefix, "<DIR>"), @r#""Source path is not a file: <DIR>/bad_0.mbtiles""#);
    }

    #[cfg(all(feature = "mlt", feature = "hillshade", feature = "_tiles"))]
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
                    FileConfigSrc::Obj(Box::new(FileConfigSource {
                        path: overridden.clone(),
                        convert_to_mlt: Some(AutoOption::Disabled),
                        convert_to_mvt: None,
                        cache_control: None,
                        convert_to_hillshade: None,
                        #[cfg(all(feature = "contour", feature = "_tiles"))]
                        convert_to_contour: None,
                        cache: CachePolicy::default(),
                    })),
                ),
                ("plain".to_owned(), FileConfigSrc::Path(plain.clone())),
            ])),
            ..FileConfig::<()>::default()
        });
        let kind_level = ProcessConfig {
            convert_to_mlt: Some(AutoOption::Auto),
            convert_to_mvt: None,
            cache_control: None,
            convert_to_hillshade: None,
            #[cfg(all(feature = "contour", feature = "_tiles"))]
            convert_to_contour: None,
        };
        let discovery = FsDiscovery::from_config(
            FileKind::Mbtiles,
            &config,
            &["mbtiles"],
            IdResolver::new(&[]),
            &kind_level,
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
                FileConfigSrc::Obj(Box::new(FileConfigSource {
                    path: pinned.clone(),
                    #[cfg(all(feature = "mlt", feature = "_tiles"))]
                    convert_to_mlt: None,
                    #[cfg(all(feature = "mlt", feature = "_tiles"))]
                    convert_to_mvt: None,
                    #[cfg(all(feature = "hillshade", feature = "_tiles"))]
                    convert_to_hillshade: None,
                    #[cfg(all(feature = "contour", feature = "_tiles"))]
                    convert_to_contour: None,
                    cache_control: Some(
                        serde_saphyr::from_str("public, max-age=60").expect("valid header"),
                    ),
                    cache: CachePolicy::default(),
                })),
            )])),
            ..FileConfig::<()>::default()
        });
        let discovery = FsDiscovery::from_config(
            FileKind::Mbtiles,
            &config,
            &["mbtiles"],
            IdResolver::new(&[]),
            &ProcessConfig::default(),
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

    #[cfg(unix)]
    #[tokio::test]
    async fn init_warns_about_an_unreadable_directory_and_publishes_its_siblings() {
        use std::os::unix::fs::PermissionsExt as _;

        let readable = tempfile::tempdir().expect("tempdir");
        File::create(readable.path().join("alpha.mbtiles")).expect("create alpha");
        let unreadable = tempfile::tempdir().expect("tempdir");
        std::fs::set_permissions(unreadable.path(), std::fs::Permissions::from_mode(0o000))
            .expect("chmod 000");

        let discovery = FsDiscovery::from_config(
            FileKind::Mbtiles,
            &FileConfigEnum::<()>::Paths(vec![
                readable.path().to_path_buf(),
                unreadable.path().to_path_buf(),
            ]),
            &["mbtiles"],
            IdResolver::new(&[]),
            &ProcessConfig::default(),
            fake_builder(),
        );
        std::fs::set_permissions(unreadable.path(), std::fs::Permissions::from_mode(0o755))
            .expect("restore permissions");

        assert_eq!(
            discovery.directories(),
            vec![readable.path().canonicalize().expect("canonicalize")],
            "only the readable directory is watched"
        );

        let catalog = TileSourceManager::new(None, OnInvalid::Warn);
        let warnings = ReloadDriver::new(discovery, catalog.clone())
            .init()
            .await
            .expect("init");

        let prefix = unreadable.path().to_string_lossy().to_string();
        let warnings: Vec<String> = warnings
            .iter()
            .map(|w| w.to_string().replace(&prefix, "<DIR>"))
            .collect();
        assert_yaml_snapshot!(warnings, @r#"- "Path <DIR>: Permission denied (os error 13)""#);
        assert_yaml_snapshot!(sorted_source_names(&catalog), @"- alpha");
    }

    #[test]
    fn config_entry_spells_paths_through_the_configured_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        File::create(dir.path().join("alpha.mbtiles")).expect("create alpha");

        let discovery = FsDiscovery::from_config(
            FileKind::Mbtiles,
            &FileConfigEnum::<()>::Path(dir.path().to_path_buf()),
            &["mbtiles"],
            IdResolver::new(&[]),
            &ProcessConfig::default(),
            unreachable_builder(),
        );

        let canonical = dir
            .path()
            .join("alpha.mbtiles")
            .canonicalize()
            .expect("canonicalize");
        let entry = discovery.config_entry(&canonical);
        assert_eq!(
            entry.get_path(),
            &dir.path().join("alpha.mbtiles"),
            "the entry keeps the configured directory's spelling"
        );
    }
}
