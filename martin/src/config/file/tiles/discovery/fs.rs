//! [`FsDiscovery`]: a [`Discovery`] over local directories, used by the file-backed kinds.
//! Each kind differs only by its extension list and a build closure.

use std::collections::BTreeMap;
use std::path::PathBuf;

use futures::future::BoxFuture;
use martin_core::tiles::BoxedSource;

use crate::MartinResult;
use crate::config::file::file_config::is_remote_url;
use crate::config::file::tiles::discovery::{Discovery, Version};
use crate::config::file::tiles::reload::discover_sources_by_ext;
use crate::config::file::{CachePolicy, FileConfigEnum, ProcessConfig};
use crate::config::primitives::{IdResolver, OptOneMany};

type BuiltSource = BoxFuture<'static, MartinResult<BoxedSource>>;

pub type FsSourceBuilder = Box<dyn Fn(String, PathBuf, CachePolicy) -> BuiltSource + Send + Sync>;

/// A [`Discovery`] that enumerates source files under watched directories.
pub struct FsDiscovery {
    directories: Vec<PathBuf>,
    extensions: &'static [&'static str],
    path_cache: BTreeMap<PathBuf, CachePolicy>,
    id_resolver: IdResolver,
    process: ProcessConfig,
    build: FsSourceBuilder,
}

impl FsDiscovery {
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
            .map(|(id, (path, modified_ms, policy))| (id, (modified_ms, (path, policy))))
            .collect())
    }

    async fn build(&self, id: &str, args: &Self::Args) -> MartinResult<BoxedSource> {
        (self.build)(id.to_string(), args.0.clone(), args.1).await
    }

    fn process(&self) -> ProcessConfig {
        self.process.clone()
    }
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

        let snapshot = discovery.discover().await.expect("discover");

        let mut ids: Vec<&String> = snapshot.keys().collect();
        ids.sort();
        assert_eq!(ids, vec!["alpha", "beta"]);
        assert!(
            snapshot.values().all(|(v, _)| *v > 0),
            "file sources carry a nonzero mtime version"
        );
    }
}
