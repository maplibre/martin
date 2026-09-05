//! Storage-neutral discovery over remote object prefixes.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::stream::TryStreamExt as _;
use object_store::ObjectStore as _;
use url::Url;

use crate::config::file::pmtiles::PmtConfig;
use crate::config::file::process::{ProcessConfig, ResolvedProcess};
use crate::config::file::source_location::SourceLocation;
use crate::config::file::tiles::discovery::{BuiltSource, Discovered, Discovery, Version};
use crate::config::file::{
    CachePolicy, ConfigFileError, FileConfigEnum, SourceBuildResult, TileSourceConfiguration,
};
use crate::config::primitives::{IdResolver, OptOneMany};

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
    Pmtiles(PmtConfig),
}

impl ObjectStoreSourceBuilder {
    async fn build(
        &self,
        id: String,
        url: Url,
        cache: CachePolicy,
    ) -> SourceBuildResult<BuiltSource> {
        match self {
            Self::Pmtiles(config) => config.new_sources_url(id, url, cache).await.map(Into::into),
        }
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

fn version_from_meta(meta: &object_store::ObjectMeta) -> Version {
    if let Some(etag) = &meta.e_tag {
        Version::Tracked(xxhash_rust::xxh3::xxh3_128(etag.as_bytes()))
    } else {
        u128::try_from(meta.last_modified.timestamp_millis())
            .map_or(Version::Opaque, Version::Tracked)
    }
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
    use object_store::memory::InMemory;
    use object_store::{ObjectStoreExt as _, PutPayload};

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
