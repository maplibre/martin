//! [`ObjectStoreDiscovery`]: a [`Discovery`] over remote object-store prefixes (`PMTiles`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use futures::stream::TryStreamExt as _;
use martin_core::tiles::BoxedSource;
use object_store::ObjectStore as _;
use url::Url;

use crate::MartinResult;
use crate::config::file::file_config::is_remote_url;
use crate::config::file::pmtiles::PmtConfig;
use crate::config::file::process::ProcessConfig;
use crate::config::file::tiles::discovery::{Discovery, Version};
use crate::config::file::{
    CachePolicy, ConfigFileError, FileConfigEnum, TileSourceConfiguration as _,
};
use crate::config::primitives::{IdResolver, OptOneMany};

const PMTILES_EXT_DOT: &str = ".pmtiles";

/// A [`Discovery`] over remote object-store prefixes; entries are [`Version::Opaque`].
pub struct ObjectStoreDiscovery {
    remote_prefixes: Vec<Url>,
    id_resolver: IdResolver,
    config: PmtConfig,
    process: ProcessConfig,
}

impl ObjectStoreDiscovery {
    /// Collects the remote URL prefixes from a file config; local paths are skipped.
    #[must_use]
    pub fn from_config(
        config: &FileConfigEnum<PmtConfig>,
        id_resolver: IdResolver,
        process: ProcessConfig,
    ) -> Self {
        let mut remote_prefixes: Vec<Url> = vec![];
        let mut collect = |path: &PathBuf| {
            if !is_remote_url(path) {
                return;
            }
            let Some(url) = path.to_str().and_then(|s| Url::parse(s).ok()) else {
                tracing::warn!(
                    "remote URL prefix {:?} could not be parsed as URL; skipping",
                    path
                );
                return;
            };
            remote_prefixes.push(url);
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

        let pmt_config = match config {
            FileConfigEnum::Config(cfg) => cfg.custom.clone(),
            _ => PmtConfig::default(),
        };

        Self {
            remote_prefixes,
            id_resolver,
            config: pmt_config,
            process,
        }
    }

    /// The remote prefixes this discovery polls.
    #[must_use]
    pub fn remote_prefixes(&self) -> &[Url] {
        &self.remote_prefixes
    }

    /// Polling cadence for the remote prefixes.
    #[must_use]
    pub fn reload_interval(&self) -> Duration {
        self.config.reload_interval
    }
}

impl Discovery for ObjectStoreDiscovery {
    type Args = Url;

    async fn discover(&self) -> MartinResult<BTreeMap<String, (Version, Self::Args)>> {
        // Per-prefix failures are logged and skipped so a transient outage doesn't flap the catalog.
        let mut out: BTreeMap<String, (Version, Url)> = BTreeMap::new();
        for prefix in &self.remote_prefixes {
            match list_remote_prefix(prefix, &self.config.options, &self.id_resolver).await {
                Ok(entries) => {
                    for (id, url) in entries {
                        out.insert(id, (Version::Opaque, url));
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "PmTilesReloader: list failed for {prefix}: {e:?}; skipping prefix this tick"
                    );
                }
            }
        }
        Ok(out)
    }

    async fn build(&self, id: &str, args: &Self::Args) -> MartinResult<BoxedSource> {
        self.config
            .new_sources_url(id.to_string(), args.clone(), CachePolicy::default())
            .await
    }

    fn process(&self) -> ProcessConfig {
        self.process.clone()
    }
}

async fn list_remote_prefix(
    prefix: &Url,
    options: &std::collections::HashMap<String, String>,
    id_resolver: &IdResolver,
) -> MartinResult<Vec<(String, Url)>> {
    let (store, base) = object_store::parse_url_opts(prefix, options)
        .map_err(|e| ConfigFileError::ObjectStoreUrlParsing(e, prefix.to_string()))?;

    let mut out = Vec::new();
    let mut stream = store.list(Some(&base));
    while let Some(meta) = stream
        .try_next()
        .await
        .map_err(|e| ConfigFileError::ObjectStoreList(e, prefix.to_string()))?
    {
        if !meta.location.as_ref().ends_with(PMTILES_EXT_DOT) {
            continue;
        }
        let stem = meta
            .location
            .filename()
            .and_then(|f| f.strip_suffix(PMTILES_EXT_DOT))
            .unwrap_or("_unknown");
        // `meta.location` is store-relative (bucket-rooted for s3/gs/azure), so we have
        // to reattach scheme+authority to round-trip through `new_sources_url`.
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
        out.push((id, object_url));
    }
    Ok(out)
}
