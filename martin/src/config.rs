use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use futures::future::try_join_all;
use log::info;
use serde::{Deserialize, Serialize};
use subst::VariableMap;

use crate::MartinError::{ConfigLoadError, ConfigParseError, ConfigWriteError, NoSources};
use crate::MartinError::{ConfigLoadError, ConfigParseError, ConfigWriteError, NoSources};
#[cfg(any(feature = "fonts", feature = "postgres"))]
use crate::OptOneMany;
#[cfg(any(
    feature = "mbtiles",
    feature = "pmtiles",
    feature = "sprites",
    feature = "cog"
))]
use crate::file_config::FileConfigEnum;
use crate::source::{TileInfoSources, TileSources};
use crate::srv::{RESERVED_KEYWORDS, SrvConfig};
#[cfg(feature = "styles")]
use crate::styles::StyleSources;
use crate::utils::{CacheValue, MainCache, OptMainCache, init_aws_lc_tls, parse_base_path};
use crate::{IdResolver, MartinResult, OptOneMany};

pub type UnrecognizedValues = HashMap<String, serde_yaml::Value>;

pub struct ServerState {
    pub cache: OptMainCache,
    pub tiles: TileSources,
    #[cfg(feature = "sprites")]
    pub sprites: crate::sprites::SpriteSources,
    #[cfg(feature = "fonts")]
    pub fonts: crate::fonts::FontSources,
    #[cfg(feature = "styles")]
    pub styles: crate::styles::StyleSources,
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub cache_size_mb: Option<u64>,

    #[serde(flatten)]
    pub srv: SrvConfig,

    #[cfg(feature = "postgres")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub postgres: OptOneMany<crate::pg::PgConfig>,

    #[cfg(feature = "pmtiles")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub pmtiles: FileConfigEnum<crate::pmtiles::PmtConfig>,

    #[cfg(feature = "mbtiles")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub mbtiles: FileConfigEnum<crate::mbtiles::MbtConfig>,

    #[cfg(feature = "cog")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub cog: FileConfigEnum<crate::cog::CogConfig>,

    #[cfg(feature = "sprites")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub sprites: FileConfigEnum<crate::sprites::SpriteConfig>,

    #[cfg(feature = "styles")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub styles: FileConfigEnum<crate::styles::StyleConfig>,

    #[cfg(feature = "fonts")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub fonts: OptOneMany<PathBuf>,

    #[serde(flatten)]
    pub unrecognized: UnrecognizedValues,
}

impl Config {
    /// Apply defaults to the config, and validate if there is a connection string
    pub fn finalize(&mut self) -> MartinResult<UnrecognizedValues> {
        let mut res = UnrecognizedValues::new();
        copy_unrecognized_config(&mut res, "", &self.unrecognized);

        if let Some(path) = &self.srv.base_path {
            self.srv.base_path = Some(parse_base_path(path)?);
        }

        #[cfg(feature = "postgres")]
        for pg in self.postgres.iter_mut() {
            res.extend(pg.finalize()?);
        }

        #[cfg(feature = "pmtiles")]
        res.extend(self.pmtiles.finalize("pmtiles."));

        #[cfg(feature = "mbtiles")]
        res.extend(self.mbtiles.finalize("mbtiles."));

        #[cfg(feature = "cog")]
        res.extend(self.cog.finalize("cog."));

        #[cfg(feature = "sprites")]
        res.extend(self.sprites.finalize("sprites."));

        #[cfg(feature = "styles")]
        res.extend(self.styles.finalize("styles."));

        // TODO: support for unrecognized fonts?
        // res.extend(self.fonts.finalize("fonts.")?);

        let is_empty = true;

        #[cfg(feature = "postgres")]
        let is_empty = is_empty && self.postgres.is_empty();

        #[cfg(feature = "pmtiles")]
        let is_empty = is_empty && self.pmtiles.is_empty();

        #[cfg(feature = "mbtiles")]
        let is_empty = is_empty && self.mbtiles.is_empty();

        #[cfg(feature = "cog")]
        let is_empty = is_empty && self.cog.is_empty();

        #[cfg(feature = "sprites")]
        let is_empty = is_empty && self.sprites.is_empty();

        #[cfg(feature = "styles")]
        let is_empty = is_empty && self.styles.is_empty();

        #[cfg(feature = "fonts")]
        let is_empty = is_empty && self.fonts.is_empty();

        if is_empty { Err(NoSources) } else { Ok(res) }
    }

    pub async fn resolve(&mut self) -> MartinResult<ServerState> {
        init_aws_lc_tls()?;
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
            sprites: crate::sprites::SpriteSources::resolve(&mut self.sprites)?,
            #[cfg(feature = "fonts")]
            fonts: crate::fonts::FontSources::resolve(&mut self.fonts)?,
            #[cfg(feature = "styles")]
            styles: crate::styles::StyleSources::resolve(&mut self.styles)?,
            cache,
        })
    }

    async fn resolve_tile_sources(
        &mut self,
        #[allow(unused_variables)] idr: &IdResolver,
        #[allow(unused_variables)] cache: OptMainCache,
    ) -> MartinResult<TileSources> {
        #[allow(unused_mut)]
        let mut sources: Vec<Pin<Box<dyn Future<Output = MartinResult<TileInfoSources>>>>> =
            Vec::new();

        #[cfg(feature = "postgres")]
        for s in self.postgres.iter_mut() {
            sources.push(Box::pin(s.resolve(idr.clone())));
        }

        #[cfg(feature = "pmtiles")]
        if !self.pmtiles.is_empty() {
            let cfg = &mut self.pmtiles;
            let val = crate::file_config::resolve_files(cfg, idr, cache.clone(), &["pmtiles"]);
            sources.push(Box::pin(val));
        }

        #[cfg(feature = "mbtiles")]
        if !self.mbtiles.is_empty() {
            let cfg = &mut self.mbtiles;
            let val = crate::file_config::resolve_files(cfg, idr, cache.clone(), &["mbtiles"]);
            sources.push(Box::pin(val));
        }

        #[cfg(feature = "cog")]
        if !self.cog.is_empty() {
            let cfg = &mut self.cog;
            let val = crate::file_config::resolve_files(cfg, idr, cache.clone(), &["tif", "tiff"]);
            sources.push(Box::pin(val));
        }

        Ok(TileSources::new(try_join_all(sources).await?))
    }

    pub fn save_to_file(&self, file_name: PathBuf) -> MartinResult<()> {
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
            match File::create(&file_name) {
                Ok(mut file) => file
                    .write_all(yaml.as_bytes())
                    .map_err(|e| ConfigWriteError(e, file_name)),
                Err(e) => Err(ConfigWriteError(e, file_name)),
            }
        }
    }
}

pub fn copy_unrecognized_config(
    result: &mut UnrecognizedValues,
    prefix: &str,
    unrecognized: &UnrecognizedValues,
) {
    result.extend(
        unrecognized
            .iter()
            .map(|(k, v)| (format!("{prefix}{k}"), v.clone())),
    );
}

/// Read config from a file
pub fn read_config<'a, M>(file_name: &Path, env: &'a M) -> MartinResult<Config>
where
    M: VariableMap<'a>,
    M::Value: AsRef<str>,
{
    let mut file = File::open(file_name).map_err(|e| ConfigLoadError(e, file_name.into()))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| ConfigLoadError(e, file_name.into()))?;
    parse_config(&contents, env, file_name)
}

pub fn parse_config<'a, M>(contents: &str, env: &'a M, file_name: &Path) -> MartinResult<Config>
where
    M: VariableMap<'a>,
    M::Value: AsRef<str>,
{
    subst::yaml::from_str(contents, env).map_err(|e| ConfigParseError(e, file_name.into()))
}

#[cfg(feature = "postgres")]
#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tests::FauxEnv;

    pub fn parse_cfg(yaml: &str) -> Config {
        parse_config(yaml, &FauxEnv::default(), Path::new("<test>")).unwrap()
    }

    pub fn assert_config(yaml: &str, expected: &Config) {
        let mut config = parse_cfg(yaml);
        let res = config.finalize().unwrap();
        assert!(res.is_empty(), "unrecognized config: {res:?}");
        assert_eq!(&config, expected);
    }
}
