use std::ffi::OsStr;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
use std::sync::LazyLock;

use futures::future::{BoxFuture, try_join_all};
use log::{info, warn};
use martin_core::cache::{CacheValue, MainCache, OptMainCache};
use martin_core::config::IdResolver;
#[cfg(any(feature = "fonts", feature = "postgres"))]
use martin_core::config::OptOneMany;
use martin_core::tiles::BoxedSource;
use serde::{Deserialize, Serialize};
use subst::VariableMap;

#[cfg(any(
    feature = "unstable-cog",
    feature = "mbtiles",
    feature = "pmtiles",
    feature = "sprites",
    feature = "styles",
))]
use crate::config::file::FileConfigEnum;
use crate::config::file::{
    ConfigFileError, ConfigFileResult, ConfigurationLivecycleHooks, UnrecognizedKeys,
    UnrecognizedValues, copy_unrecognized_keys_from_config,
};
use crate::source::TileSources;
use crate::srv::RESERVED_KEYWORDS;
use crate::{MartinError, MartinResult};

pub struct ServerState {
    pub cache: OptMainCache,
    pub tiles: TileSources,
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
    pub cache_size_mb: Option<u64>,

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
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
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
        let resolver = IdResolver::new(RESERVED_KEYWORDS);
        let cache_size = self.cache_size_mb.unwrap_or(512) * 1024 * 1024;
        let cache = if cache_size > 0 {
            info!("Initializing main cache with maximum size {cache_size}B");
            Some(
                MainCache::builder()
                    .weigher(|_key, value: &CacheValue| -> u32 {
                        match value {
                            CacheValue::Tile(v) => v.len().try_into().unwrap_or(u32::MAX),
                            #[cfg(feature = "pmtiles")]
                            CacheValue::PmtDirectory(v) => {
                                v.get_approx_byte_size().try_into().unwrap_or(u32::MAX)
                            }
                        }
                    })
                    .max_capacity(cache_size)
                    .build(),
            )
        } else {
            info!("Caching is disabled");
            None
        };

        Ok(ServerState {
            tiles: self.resolve_tile_sources(&resolver, cache.clone()).await?,
            #[cfg(feature = "sprites")]
            sprites: self.sprites.resolve()?,
            #[cfg(feature = "fonts")]
            fonts: self.fonts.resolve()?,
            #[cfg(feature = "styles")]
            styles: self.styles.resolve()?,
            cache,
        })
    }

    async fn resolve_tile_sources(
        &mut self,
        #[allow(unused_variables)] idr: &IdResolver,
        #[allow(unused_variables)] cache: OptMainCache,
    ) -> MartinResult<TileSources> {
        #[allow(unused_mut)]
        let mut sources: Vec<BoxFuture<MartinResult<Vec<BoxedSource>>>> = Vec::new();

        #[cfg(feature = "postgres")]
        for s in self.postgres.iter_mut() {
            sources.push(Box::pin(s.resolve(idr.clone())));
        }

        #[cfg(feature = "pmtiles")]
        if !self.pmtiles.is_empty() {
            let cfg = &mut self.pmtiles;
            let val = crate::config::file::resolve_files(cfg, idr, cache.clone(), &["pmtiles"]);
            sources.push(Box::pin(val));
        }

        #[cfg(feature = "mbtiles")]
        if !self.mbtiles.is_empty() {
            let cfg = &mut self.mbtiles;
            let val = crate::config::file::resolve_files(cfg, idr, cache.clone(), &["mbtiles"]);
            sources.push(Box::pin(val));
        }

        #[cfg(feature = "unstable-cog")]
        if !self.cog.is_empty() {
            let cfg = &mut self.cog;
            let val = crate::config::file::resolve_files(cfg, idr, cache.clone(), &["tif", "tiff"]);
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

fn init_aws_lc_tls() {
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
