use crate::config::file::pmtiles::PmtConfig;
use crate::config::file::process::ProcessConfig;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::resolve_process_config;
use crate::config::file::tiles::discovery::{FsDiscovery, FsSourceBuilder, ObjectStoreDiscovery};
use crate::config::file::tiles::driver::{Baseline, NotifyTrigger, PollTrigger, ReloadDriver};
use crate::config::file::{FileConfigEnum, TileSourceConfiguration as _};
use crate::config::primitives::IdResolver;
use crate::{MartinResult, TileSourceManager};

const PMTILES_EXT: &str = "pmtiles";

/// Reloader for `PMTiles` sources.
///
/// Local directories use a [`NotifyTrigger`] for sub-second feedback; remote URL prefixes
/// (`s3://`, `gs://`, `https://`, …) use a [`PollTrigger`] because blob stores have no event
/// channel. Each half is its own [`ReloadDriver`] so neither needs a shared mutex.
pub struct PmTilesReloader {
    tile_source_manager: TileSourceManager,
    local: FsDiscovery,
    remote: ObjectStoreDiscovery,
}

impl PmTilesReloader {
    #[must_use]
    pub fn new(
        tsm: TileSourceManager,
        id_resolver: IdResolver,
        config: &FileConfigEnum<PmtConfig>,
        global_process: &ProcessConfig,
    ) -> Self {
        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        let process = {
            let source_type = match config {
                FileConfigEnum::Config(cfg) => ProcessConfig {
                    convert_to_mlt: cfg.custom.convert_to_mlt.clone(),
                    convert_to_mvt: cfg.custom.convert_to_mvt.clone(),
                },
                _ => ProcessConfig::default(),
            };
            resolve_process_config(global_process, &source_type, &ProcessConfig::default())
        };
        #[cfg(not(feature = "mlt"))]
        let process = {
            let _ = global_process;
            ProcessConfig::default()
        };

        let pmt_config = match config {
            FileConfigEnum::Config(cfg) => cfg.custom.clone(),
            _ => PmtConfig::default(),
        };

        // Local sources are built through `PmtConfig::new_sources` (path -> file:// URL).
        let build_config = pmt_config.clone();
        let build: FsSourceBuilder = Box::new(move |id, path, policy| {
            let config = build_config.clone();
            Box::pin(async move { config.new_sources(id, path, policy).await })
        });
        let local = FsDiscovery::from_config(
            config,
            &[PMTILES_EXT],
            id_resolver.clone(),
            process.clone(),
            build,
        );
        let remote = ObjectStoreDiscovery::from_config(config, id_resolver, process);

        Self {
            tile_source_manager: tsm,
            local,
            remote,
        }
    }

    pub fn start(self) -> MartinResult<()> {
        let Self {
            tile_source_manager,
            local,
            remote,
        } = self;

        let directories = local.directories().to_vec();
        let has_remote = !remote.remote_prefixes().is_empty();
        let interval = remote.reload_interval();

        if directories.is_empty() && !has_remote {
            return Ok(());
        }

        if !directories.is_empty() {
            let trigger = NotifyTrigger::new(&directories)?;
            ReloadDriver::new(local, tile_source_manager.clone())
                .spawn(trigger, Baseline::StartupResolved);
        }

        if has_remote {
            if interval.is_zero() {
                tracing::info!(
                    "PmTilesReloader: remote prefix polling disabled (reload_interval = 0s)"
                );
            } else {
                let trigger = PollTrigger::new(interval);
                ReloadDriver::new(remote, tile_source_manager).spawn(trigger, Baseline::Empty);
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

    fn make_reloader(config: &FileConfigEnum<PmtConfig>) -> PmTilesReloader {
        let tsm = TileSourceManager::new(None, OnInvalid::Warn);
        let resolver = IdResolver::new(&[]);
        PmTilesReloader::new(tsm, resolver, config, &ProcessConfig::default())
    }

    #[derive(serde::Serialize)]
    struct ReloaderSnapshot {
        local_dir_count: usize,
        remote_prefix_count: usize,
        remote_prefixes: Vec<String>,
        interval_secs: u64,
    }

    impl From<&PmTilesReloader> for ReloaderSnapshot {
        fn from(r: &PmTilesReloader) -> Self {
            Self {
                local_dir_count: r.local.directories().len(),
                remote_prefix_count: r.remote.remote_prefixes().len(),
                remote_prefixes: r
                    .remote
                    .remote_prefixes()
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                interval_secs: r.remote.reload_interval().as_secs(),
            }
        }
    }

    #[test]
    fn new_with_none_config_yields_default_interval() {
        let reloader = make_reloader(&FileConfigEnum::None);
        assert!(reloader.local.directories().is_empty());
        assert!(reloader.remote.remote_prefixes().is_empty());
        assert_eq!(reloader.remote.reload_interval(), DEFAULT_RELOAD_INTERVAL);
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
        assert_eq!(r.remote.remote_prefixes().len(), 1);
    }

    #[test]
    fn new_skips_remote_individually_configured_sources() {
        let mut sources: BTreeMap<String, FileConfigSrc> = BTreeMap::new();
        sources.insert(
            "remote_a".to_string(),
            FileConfigSrc::Obj(FileConfigSource {
                path: PathBuf::from("s3://bucket/file.pmtiles"),
                cache: CachePolicy::default(),
                #[cfg(all(feature = "mlt", feature = "_tiles"))]
                convert_to_mlt: None,
                #[cfg(all(feature = "mlt", feature = "_tiles"))]
                convert_to_mvt: None,
            }),
        );
        let cfg = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::NoVals,
            sources: Some(sources),
            custom: PmtConfig::default(),
        });
        let r = make_reloader(&cfg);
        // Remote single-file sources are tracked elsewhere (resolve_files) -- the reloader
        // does not need to re-list them, so neither half picks them up.
        assert!(r.local.directories().is_empty());
        assert!(r.remote.remote_prefixes().is_empty());
    }
}
