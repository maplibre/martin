//! [`FsDiscovery`]: a [`Discovery`] over local directories, used by the file-backed kinds.
//! Each kind differs only by its extension list and a build closure.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use futures::future::BoxFuture;
use martin_core::tiles::BoxedSource;
use tokio::fs::{self, DirEntry};

use crate::config::file::file_config::is_remote_url;
use crate::config::file::tiles::discovery::{Discovery, Version};
use crate::config::file::{CachePolicy, FileConfigEnum, ProcessConfig};
use crate::config::primitives::{IdResolver, OptOneMany};
use crate::{MartinError, MartinResult};

/// The future an [`FsSourceBuilder`] returns: the freshly-built source, or an init error.
type BuiltSource = BoxFuture<'static, MartinResult<BoxedSource>>;

/// Opens one discovered file as a source.
/// Both builders are non-capturing, so a `fn` pointer avoids a boxed `dyn Fn`.
pub type FsSourceBuilder = fn(String, PathBuf, CachePolicy) -> BuiltSource;

/// A [`Discovery`] that enumerates source files under the watched directories.
pub struct FsDiscovery {
    directories: Vec<PathBuf>,
    extensions: &'static [&'static str],
    /// Canonical path -> policy for configured sources, so discovered files inherit their policy.
    path_cache: BTreeMap<PathBuf, CachePolicy>,
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
        let mut path_cache: BTreeMap<PathBuf, CachePolicy> = BTreeMap::new();

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
                path_cache.insert(canonical, src.cache_zoom());
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
            path_cache,
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

impl Discovery for FsDiscovery {
    type Args = (PathBuf, CachePolicy);

    async fn discover(&self) -> MartinResult<BTreeMap<String, (Version, Self::Args)>> {
        let discovered = discover_sources_by_ext(
            &self.directories,
            self.extensions,
            &self.path_cache,
            &self.id_resolver,
        )
        .await?;

        Ok(discovered
            .into_iter()
            .map(|(id, (path, modified_at_ms, policy))| {
                (id, (Version::Tracked(modified_at_ms), (path, policy)))
            })
            .collect())
    }

    async fn build(&self, id: &str, args: &Self::Args) -> MartinResult<BoxedSource> {
        (self.build)(id.to_string(), args.0.clone(), args.1).await
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
        stem: stem.to_string(),
        path_str,
        modified_ms,
    })
}

/// Scans `directories` for files matching `extensions`, resolving ids and cache policies.
async fn discover_sources_by_ext(
    directories: &[PathBuf],
    extensions: &[&str],
    path_cache: &BTreeMap<PathBuf, CachePolicy>,
    id_resolver: &IdResolver,
) -> MartinResult<BTreeMap<String, (PathBuf, u128, CachePolicy)>> {
    let mut out = BTreeMap::new();
    for directory in directories {
        let mut entries = fs::read_dir(directory)
            .await
            .map_err(MartinError::IoError)?;
        while let Some(entry) = entries.next_entry().await.map_err(MartinError::IoError)? {
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
            let policy = path_cache.get(&e.path).copied().unwrap_or_default();
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
        |id, _path, _policy| {
            Box::pin(async move { panic!("build should not be called by discover(): {id}") })
        }
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

        let snapshot = discovery.discover().await.expect("discover");

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
}
