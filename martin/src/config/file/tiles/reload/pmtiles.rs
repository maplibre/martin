use crate::TileSourceManager;
use crate::config::file::pmtiles::PmtConfig;
use crate::config::file::process::ProcessConfig;
#[cfg(feature = "_process")]
use crate::config::file::resolve_process_config;
use crate::config::file::tiles::discovery::{FsDiscovery, FsSourceBuilder, ObjectStoreDiscovery};
use crate::config::file::tiles::driver::{Baseline, NotifyTrigger, PollTrigger, ReloadDriver};
use crate::config::file::{
    FileConfigEnum, SourceBuildResult, TileSourceConfiguration as _, TileSourceWarning,
};
use crate::config::primitives::IdResolver;
use crate::reload::FileKind;

const PMTILES_EXT: &str = "pmtiles";

/// Reloader for `PMTiles` sources.
///
/// Local directories use a [`NotifyTrigger`] for sub-second feedback; remote URL prefixes
/// (`s3://`, `gs://`, `https://`, …) use a [`PollTrigger`] because blob stores have no event
/// channel. Each half is its own [`ReloadDriver`] so neither needs a shared mutex.
pub struct PmtilesReloader {
    local: ReloadDriver<FsDiscovery, TileSourceManager>,
    remote: ReloadDriver<ObjectStoreDiscovery, TileSourceManager>,
}

impl PmtilesReloader {
    #[must_use]
    pub fn new(
        tsm: TileSourceManager,
        id_resolver: IdResolver,
        config: &FileConfigEnum<PmtConfig>,
        global_process: &ProcessConfig,
    ) -> Self {
        #[cfg(feature = "_process")]
        let process = {
            let source_type = match config {
                FileConfigEnum::Config(cfg) => ProcessConfig {
                    #[cfg(feature = "mlt")]
                    convert_to_mlt: cfg.custom.convert_to_mlt.clone(),
                    #[cfg(feature = "mlt")]
                    convert_to_mvt: cfg.custom.convert_to_mvt.clone(),
                    ..Default::default()
                },
                FileConfigEnum::None | FileConfigEnum::Path(_) | FileConfigEnum::Paths(_) => {
                    ProcessConfig::default()
                }
            };
            resolve_process_config(global_process, &source_type, &ProcessConfig::default())
        };
        #[cfg(not(feature = "_process"))]
        let process = {
            let _ = global_process;
            ProcessConfig::default()
        };

        let pmt_config = match config {
            FileConfigEnum::Config(cfg) => cfg.custom.clone(),
            FileConfigEnum::None | FileConfigEnum::Path(_) | FileConfigEnum::Paths(_) => {
                PmtConfig::default()
            }
        };

        // Local sources are built through `PmtConfig::new_sources` (path -> file:// URL).
        // This closure captures `build_config` so every discovered file reuses the same shared directory cache and `object_store` options.
        // That capture is why `FsSourceBuilder` is a boxed `dyn Fn` rather than a bare `fn` pointer.
        let build_config = pmt_config.clone();
        let build: FsSourceBuilder = Box::new(move |id, path, policy| {
            let config = build_config.clone();
            Box::pin(async move { config.new_sources(id, path, policy).await })
        });
        let local = FsDiscovery::from_config(
            FileKind::Pmtiles,
            config,
            &[PMTILES_EXT],
            id_resolver.clone(),
            process.clone(),
            build,
        );
        let remote = ObjectStoreDiscovery::from_config(config, id_resolver, process);

        Self {
            local: ReloadDriver::new(local, tsm.clone()),
            remote: ReloadDriver::new(remote, tsm),
        }
    }

    /// Publishes every discovered local source into the catalog and returns the discovery warnings.
    /// Remote sources are not initialized here.
    /// They start from an empty baseline in `start()`.
    pub async fn init(&mut self) -> SourceBuildResult<Vec<TileSourceWarning>> {
        self.local.init().await
    }

    pub fn start(self) -> notify::Result<()> {
        let Self { local, remote } = self;

        let directories = local.discovery().directories();
        let has_remote = !remote.discovery().remote_prefixes().is_empty();
        let interval = remote.discovery().reload_interval();

        if directories.is_empty() && !has_remote {
            return Ok(());
        }

        if !directories.is_empty() {
            let trigger = NotifyTrigger::new(&directories)?;
            local.spawn(trigger, Baseline::Initialized);
        }

        if has_remote {
            if interval.is_zero() {
                tracing::info!(
                    "PmtilesReloader: remote prefix polling disabled (reload_interval = 0s)"
                );
            } else {
                let trigger = PollTrigger::new(interval);
                remote.spawn(trigger, Baseline::Empty);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::Duration;

    use insta::assert_yaml_snapshot;

    use super::*;
    use crate::config::file::pmtiles::DEFAULT_RELOAD_INTERVAL;
    use crate::config::file::{
        CachePolicy, FileConfig, FileConfigSource, FileConfigSrc, OnInvalid,
    };
    use crate::config::primitives::OptOneMany;

    fn make_reloader(config: &FileConfigEnum<PmtConfig>) -> PmtilesReloader {
        let tsm = TileSourceManager::new(None, OnInvalid::Warn);
        let resolver = IdResolver::new(&[]);
        PmtilesReloader::new(tsm, resolver, config, &ProcessConfig::default())
    }

    #[derive(serde::Serialize)]
    struct ReloaderSnapshot {
        local_dir_count: usize,
        remote_prefix_count: usize,
        remote_prefixes: Vec<String>,
        interval_secs: u64,
    }

    impl From<&PmtilesReloader> for ReloaderSnapshot {
        fn from(r: &PmtilesReloader) -> Self {
            Self {
                local_dir_count: r.local.discovery().directories().len(),
                remote_prefix_count: r.remote.discovery().remote_prefixes().len(),
                remote_prefixes: r
                    .remote
                    .discovery()
                    .remote_prefixes()
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                interval_secs: r.remote.discovery().reload_interval().as_secs(),
            }
        }
    }

    #[test]
    fn new_with_none_config_yields_default_interval() {
        let reloader = make_reloader(&FileConfigEnum::None);
        assert!(reloader.local.discovery().directories().is_empty());
        assert!(reloader.remote.discovery().remote_prefixes().is_empty());
        assert_eq!(
            reloader.remote.discovery().reload_interval(),
            DEFAULT_RELOAD_INTERVAL
        );
    }

    #[test]
    fn new_partitions_local_and_remote_paths() {
        let cfg = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::Many(vec![
                PathBuf::from("s3://bucket-a/"),
                PathBuf::from("s3://bucket-b/folder/"),
                PathBuf::from("https://example.com/tiles/"),
            ]),
            sources: None,
            custom: PmtConfig {
                reload_interval: Duration::from_secs(30),
                ..PmtConfig::default()
            },
        });
        assert_yaml_snapshot!(ReloaderSnapshot::from(&make_reloader(&cfg)), @r#"
        local_dir_count: 0
        remote_prefix_count: 3
        remote_prefixes:
          - "https://example.com/tiles/"
          - "s3://bucket-a/"
          - "s3://bucket-b/folder/"
        interval_secs: 30
        "#);
    }

    #[test]
    fn new_dedups_remote_prefixes() {
        let cfg = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::Many(vec![
                PathBuf::from("s3://bucket/"),
                PathBuf::from("s3://bucket/"),
            ]),
            sources: None,
            custom: PmtConfig::default(),
        });
        let r = make_reloader(&cfg);
        assert_eq!(r.remote.discovery().remote_prefixes().len(), 1);
    }

    #[test]
    fn new_skips_remote_individually_configured_sources() {
        let mut sources: BTreeMap<String, FileConfigSrc> = BTreeMap::new();
        sources.insert(
            "remote_a".to_owned(),
            FileConfigSrc::Obj(Box::new(FileConfigSource {
                path: PathBuf::from("s3://bucket/file.pmtiles"),
                cache: CachePolicy::default(),
                #[cfg(feature = "mlt")]
                convert_to_mlt: None,
                #[cfg(feature = "mlt")]
                convert_to_mvt: None,
                cache_control: None,
                #[cfg(all(feature = "hillshade", feature = "_tiles"))]
                convert_to_hillshade: None,
            })),
        );
        let cfg = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::NoVals,
            sources: Some(sources),
            custom: PmtConfig::default(),
        });
        let r = make_reloader(&cfg);
        // Remote single-file sources are tracked elsewhere (resolve_files) -- the reloader
        // does not need to re-list them, so neither half picks them up.
        assert!(r.local.discovery().directories().is_empty());
        assert!(r.remote.discovery().remote_prefixes().is_empty());
    }
}
