use std::ffi::OsStr;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
use std::sync::LazyLock;

#[cfg(feature = "_tiles")]
use futures::future::{BoxFuture, try_join_all};
use log::{info, warn};
#[cfg(feature = "_tiles")]
use martin_core::config::IdResolver;
#[cfg(feature = "postgres")]
use martin_core::config::OptOneMany;
#[cfg(feature = "pmtiles")]
use martin_core::tiles::pmtiles::PmtCache;
#[cfg(feature = "_tiles")]
use martin_core::tiles::{BoxedSource, OptTileCache};
use serde::{Deserialize, Serialize};
use subst::VariableMap;

#[cfg(any(
    feature = "pmtiles",
    feature = "mbtiles",
    feature = "unstable-cog",
    feature = "styles",
    feature = "sprites",
    feature = "fonts",
))]
use crate::config::file::FileConfigEnum;
#[cfg(feature = "_tiles")]
use crate::config::file::cache::CacheConfig;
use crate::config::file::{
    ConfigFileError, ConfigFileResult, ConfigurationLivecycleHooks, UnrecognizedKeys,
    UnrecognizedValues, copy_unrecognized_keys_from_config,
};
#[cfg(feature = "_tiles")]
use crate::source::TileSources;
#[cfg(feature = "_tiles")]
use crate::srv::RESERVED_KEYWORDS;
use crate::{MartinError, MartinResult};

pub struct ServerState {
    #[cfg(feature = "_tiles")]
    pub tiles: TileSources,
    #[cfg(feature = "_tiles")]
    pub tile_cache: OptTileCache,

    #[cfg(feature = "sprites")]
    pub sprites: martin_core::sprites::SpriteSources,

    #[cfg(feature = "fonts")]
    pub fonts: martin_core::fonts::FontSources,

    #[cfg(feature = "styles")]
    pub styles: martin_core::styles::StyleSources,
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Maximum size of the tile cache in megabytes (0 to disable)
    ///
    /// Can be overridden by [`tile_cache_size_mb`](Self::tile_cache_size_mb) or similar configuration options.
    pub cache_size_mb: Option<u64>,
    /// Maximum size of the tile cache in megabytes (0 to disable)
    ///
    /// Overrides [`cache_size_mb`](Self::cache_size_mb)
    pub tile_cache_size_mb: Option<u64>,

    #[serde(flatten)]
    pub srv: super::srv::SrvConfig,

    #[cfg(feature = "postgres")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub postgres: OptOneMany<super::postgres::PostgresConfig>,

    #[cfg(feature = "pmtiles")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub pmtiles: FileConfigEnum<super::pmtiles::PmtConfig>,

    #[cfg(feature = "mbtiles")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub mbtiles: FileConfigEnum<super::mbtiles::MbtConfig>,

    #[cfg(feature = "unstable-cog")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub cog: FileConfigEnum<super::cog::CogConfig>,

    #[cfg(feature = "sprites")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub sprites: super::sprites::SpriteConfig,

    #[cfg(feature = "styles")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub styles: super::styles::StyleConfig,

    #[cfg(feature = "fonts")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub fonts: super::fonts::FontConfig,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl Config {
    /// Apply defaults to the config, and validate if there is a connection string
    pub fn finalize(&mut self) -> MartinResult<UnrecognizedKeys> {
        let mut res = self.srv.get_unrecognized_keys();
        copy_unrecognized_keys_from_config(&mut res, "", &self.unrecognized);

        if let Some(path) = &self.srv.base_path {
            self.srv.base_path = Some(parse_base_path(path)?);
        }
        #[cfg(feature = "postgres")]
        {
            let pg_prefix = if matches!(self.postgres, OptOneMany::One(_)) {
                "postgres."
            } else {
                "postgres[]."
            };
            for pg in self.postgres.iter_mut() {
                pg.finalize()?;
                res.extend(pg.get_unrecognized_keys_with_prefix(pg_prefix));
            }
        }

        #[cfg(feature = "pmtiles")]
        {
            // if a pmtiles source were to keep being configured like this,
            // we would not be able to migrate defaults/deprecate settings
            //
            // pmiles intialisation after this in resolve_tile_sources depends on this behaviour and will panic otherwise
            self.pmtiles = self.pmtiles.clone().into_config();
            self.pmtiles.finalize()?;
            res.extend(self.pmtiles.get_unrecognized_keys_with_prefix("pmtiles."));
        }

        #[cfg(feature = "mbtiles")]
        {
            self.mbtiles.finalize()?;
            res.extend(self.mbtiles.get_unrecognized_keys_with_prefix("mbtiles."));
        }

        #[cfg(feature = "unstable-cog")]
        {
            self.cog.finalize()?;
            res.extend(self.cog.get_unrecognized_keys_with_prefix("cog."));
        }

        #[cfg(feature = "sprites")]
        {
            self.sprites.finalize()?;
            res.extend(self.sprites.get_unrecognized_keys_with_prefix("sprites."));
        }

        #[cfg(feature = "styles")]
        {
            self.styles.finalize()?;
            res.extend(self.styles.get_unrecognized_keys_with_prefix("styles."));
        }

        // TODO: support for unrecognized fonts?
        // #[cfg(feature = "fonts")]
        // {
        //     self.fonts.finalize()?;
        //     res.extend(self.fonts.get_unrecognized_keys_with_prefix("fonts."));
        // }

        for key in &res {
            warn!(
                "Ignoring unrecognized configuration key '{key}'. Please check your configuration file for typos."
            );
        }

        let is_empty = true;

        #[cfg(feature = "postgres")]
        let is_empty = is_empty && self.postgres.is_empty();

        #[cfg(feature = "pmtiles")]
        let is_empty = is_empty && self.pmtiles.is_empty();

        #[cfg(feature = "mbtiles")]
        let is_empty = is_empty && self.mbtiles.is_empty();

        #[cfg(feature = "unstable-cog")]
        let is_empty = is_empty && self.cog.is_empty();

        #[cfg(feature = "sprites")]
        let is_empty = is_empty && self.sprites.is_empty();

        #[cfg(feature = "styles")]
        let is_empty = is_empty && self.styles.is_empty();

        #[cfg(feature = "fonts")]
        let is_empty = is_empty && self.fonts.is_empty();

        if is_empty {
            Err(ConfigFileError::NoSources.into())
        } else {
            Ok(res)
        }
    }

    pub async fn resolve(&mut self) -> MartinResult<ServerState> {
        init_aws_lc_tls();

        #[cfg(feature = "_tiles")]
        let resolver = IdResolver::new(RESERVED_KEYWORDS);

        #[cfg(feature = "_tiles")]
        let cache_config = self.resolve_cache_config();

        #[cfg(feature = "pmtiles")]
        let pmtiles_cache = cache_config.create_pmtiles_cache();

        Ok(ServerState {
            #[cfg(feature = "_tiles")]
            tiles: self
                .resolve_tile_sources(
                    &resolver,
                    #[cfg(feature = "pmtiles")]
                    pmtiles_cache,
                )
                .await?,
            #[cfg(feature = "_tiles")]
            tile_cache: cache_config.create_tile_cache(),

            #[cfg(feature = "sprites")]
            sprites: self.sprites.resolve()?,

            #[cfg(feature = "fonts")]
            fonts: self.fonts.resolve()?,

            #[cfg(feature = "styles")]
            styles: self.styles.resolve()?,
        })
    }

    #[cfg(feature = "_tiles")]
    // cache_config is still respected, but can be overridden by individual cache sizes
    //
    // `cache_config: 0` disables caching, unless overridden by individual cache sizes
    fn resolve_cache_config(&self) -> CacheConfig {
        if let Some(cache_size_mb) = self.cache_size_mb {
            #[cfg(feature = "pmtiles")]
            let pmtiles_cache_size_mb = if let FileConfigEnum::Config(cfg) = &self.pmtiles {
                cfg.custom
                    .directory_cache_size_mb
                    .unwrap_or(cache_size_mb / 4) // Default: 25% for PMTiles directories
            } else {
                cache_size_mb / 4 // Default: 25% for PMTiles directories
            };

            CacheConfig {
                #[cfg(feature = "_tiles")]
                tile_cache_size_mb: self.tile_cache_size_mb.unwrap_or(cache_size_mb / 2), // Default: 50% for tiles
                #[cfg(feature = "pmtiles")]
                pmtiles_cache_size_mb,
            }
        } else {
            // TODO: the defaults could be smarter. If I don't have pmtiles sources, don't reserve cache for it
            CacheConfig {
                #[cfg(feature = "_tiles")]
                tile_cache_size_mb: 256,
                #[cfg(feature = "pmtiles")]
                pmtiles_cache_size_mb: 128,
            }
        }
    }

    #[cfg(feature = "_tiles")]
    async fn resolve_tile_sources(
        &mut self,
        #[allow(unused_variables)] idr: &IdResolver,
        #[cfg(feature = "pmtiles")] pmtiles_cache: PmtCache,
    ) -> MartinResult<TileSources> {
        let mut sources: Vec<BoxFuture<MartinResult<Vec<BoxedSource>>>> = Vec::new();

        #[cfg(feature = "postgres")]
        for s in self.postgres.iter_mut() {
            sources.push(Box::pin(s.resolve(idr.clone())));
        }

        #[cfg(feature = "pmtiles")]
        if !self.pmtiles.is_empty() {
            let cfg = &mut self.pmtiles;
            match cfg {
                FileConfigEnum::None => {}
                FileConfigEnum::Paths(_) | FileConfigEnum::Path(_) => unreachable!(
                    "pmtiles was transformed to FileConfigEnum::Config in the previous step via `into_config`",
                ),
                FileConfigEnum::Config(file_config) => {
                    file_config.custom.pmtiles_directory_cache = pmtiles_cache;
                }
            }
            let val = crate::config::file::resolve_files(cfg, idr, &["pmtiles"]);
            sources.push(Box::pin(val));
        }

        #[cfg(feature = "mbtiles")]
        if !self.mbtiles.is_empty() {
            let cfg = &mut self.mbtiles;
            let val = crate::config::file::resolve_files(cfg, idr, &["mbtiles"]);
            sources.push(Box::pin(val));
        }

        #[cfg(feature = "unstable-cog")]
        if !self.cog.is_empty() {
            let cfg = &mut self.cog;
            let val = crate::config::file::resolve_files(cfg, idr, &["tif", "tiff"]);
            sources.push(Box::pin(val));
        }

        Ok(TileSources::new(try_join_all(sources).await?))
    }

    pub fn save_to_file(&self, file_name: &Path) -> ConfigFileResult<()> {
        let yaml = serde_yaml::to_string(&self).expect("Unable to serialize config");
        if file_name.as_os_str() == OsStr::new("-") {
            info!("Current system configuration:");
            println!("\n\n{yaml}\n");
            Ok(())
        } else {
            info!(
                "Saving config to {}, use --config to load it",
                file_name.display()
            );
            File::create(file_name)
                .map_err(|e| ConfigFileError::ConfigWriteError(e, file_name.to_path_buf()))?
                .write_all(yaml.as_bytes())
                .map_err(|e| ConfigFileError::ConfigWriteError(e, file_name.to_path_buf()))?;
            Ok(())
        }
    }
}

/// Read config from a file
pub fn read_config<'a, M>(file_name: &Path, env: &'a M) -> ConfigFileResult<Config>
where
    M: VariableMap<'a>,
    M::Value: AsRef<str>,
{
    let mut file =
        File::open(file_name).map_err(|e| ConfigFileError::ConfigLoadError(e, file_name.into()))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| ConfigFileError::ConfigLoadError(e, file_name.into()))?;
    parse_config(&contents, env, file_name)
}

pub fn parse_config<'a, M>(contents: &str, env: &'a M, file_name: &Path) -> ConfigFileResult<Config>
where
    M: VariableMap<'a>,
    M::Value: AsRef<str>,
{
    subst::yaml::from_str(contents, env)
        .map_err(|e| ConfigFileError::ConfigParseError(e, file_name.into()))
}

pub fn parse_base_path(path: &str) -> MartinResult<String> {
    if !path.starts_with('/') {
        return Err(MartinError::BasePathError(path.to_string()));
    }
    if let Ok(uri) = path.parse::<actix_web::http::Uri>() {
        return Ok(uri.path().trim_end_matches('/').to_string());
    }
    Err(MartinError::BasePathError(path.to_string()))
}

pub fn init_aws_lc_tls() {
    // https://github.com/rustls/rustls/issues/1877
    static INIT_TLS: LazyLock<()> = LazyLock::new(|| {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("Unable to init rustls: {e:?}");
    });
    *INIT_TLS;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_base_path_accepts_valid_paths() {
        assert_eq!("", parse_base_path("/").unwrap());
        assert_eq!("", parse_base_path("//").unwrap());
        assert_eq!("/foo/bar", parse_base_path("/foo/bar").unwrap());
        assert_eq!("/foo/bar", parse_base_path("/foo/bar/").unwrap());
    }

    #[test]
    fn parse_base_path_rejects_invalid_paths() {
        assert!(parse_base_path("").is_err());
        assert!(parse_base_path("foo/bar").is_err());
    }
}
