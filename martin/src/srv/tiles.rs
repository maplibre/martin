use actix_http::ContentEncoding;
use actix_web::error::{ErrorBadRequest, ErrorNotFound};
use actix_web::http::header::{
    AcceptEncoding, Encoding as HeaderEnc, Preference, CONTENT_ENCODING,
};
use actix_web::web::{Data, Path, Query};
use actix_web::{route, HttpMessage, HttpRequest, HttpResponse, Result as ActixResult};
use futures::future::try_join_all;
use log::trace;
use martin_tile_utils::{Encoding, Format, TileInfo};
use serde::Deserialize;

use crate::args::PreferredEncoding;
use crate::source::{Source, TileSources, UrlQuery};
use crate::srv::server::map_internal_error;
use crate::srv::SrvConfig;
use crate::utils::cache::get_or_insert_cached_value;
use crate::utils::{
    decode_brotli, decode_gzip, encode_brotli, encode_gzip, CacheKey, CacheValue, MainCache,
    OptMainCache,
};
use crate::{Tile, TileCoord, TileData};

#[derive(Deserialize, Clone)]
pub struct TileRequest {
    source_ids: String,
    z: u8,
    x: u32,
    y: u32,
}

#[route("/{source_ids}/{z}/{x}/{y}", method = "GET", method = "HEAD")]
async fn get_tile(
    req: HttpRequest,
    srv_config: Data<SrvConfig>,
    path: Path<TileRequest>,
    sources: Data<TileSources>,
    cache: Data<OptMainCache>,
) -> ActixResult<HttpResponse> {
    let src = DynTileSource::new(
        sources.as_ref(),
        &path.source_ids,
        Some(path.z),
        req.query_string(),
        req.get_header::<AcceptEncoding>(),
        srv_config.preferred_encoding,
        cache.as_ref().as_ref(),
    )?;

    src.get_http_response(TileCoord {
        z: path.z,
        x: path.x,
        y: path.y,
    })
    .await
}

pub struct DynTileSource<'a> {
    pub sources: Vec<&'a dyn Source>,
    pub info: TileInfo,
    pub query_str: Option<&'a str>,
    pub query_obj: Option<UrlQuery>,
    pub accept_encodings: Option<AcceptEncoding>,
    pub prefered_encodings: Option<PreferredEncoding>,
    pub cache: Option<&'a MainCache>,
}

impl<'a> DynTileSource<'a> {
    pub fn new(
        sources: &'a TileSources,
        source_ids: &str,
        zoom: Option<u8>,
        query: &'a str,
        accpect_encodings: Option<AcceptEncoding>,
        preferred_encoding: Option<PreferredEncoding>,
        cache: Option<&'a MainCache>,
    ) -> ActixResult<Self> {
        let (sources, use_url_query, info) = sources.get_sources(source_ids, zoom)?;

        if sources.is_empty() {
            return Err(ErrorNotFound("No valid sources found"));
        }

        let mut query_obj = None;
        let mut query_str = None;
        if use_url_query && !query.is_empty() {
            query_obj = Some(Query::<UrlQuery>::from_query(query)?.into_inner());
            query_str = Some(query);
        }

        Ok(Self {
            sources,
            info,
            query_str,
            query_obj,
            accept_encodings: accpect_encodings,
            prefered_encodings: preferred_encoding,
            cache,
        })
    }

    pub async fn get_http_response(&self, xyz: TileCoord) -> ActixResult<HttpResponse> {
        let tile = self.get_tile_content(xyz).await?;

        Ok(if tile.data.is_empty() {
            HttpResponse::NoContent().finish()
        } else {
            let mut response = HttpResponse::Ok();
            response.content_type(tile.info.format.content_type());
            if let Some(val) = tile.info.encoding.content_encoding() {
                response.insert_header((CONTENT_ENCODING, val));
            }
            response.body(tile.data)
        })
    }

    pub async fn get_tile_content(&self, xyz: TileCoord) -> ActixResult<Tile> {
        let mut tiles = try_join_all(self.sources.iter().map(|s| async {
            get_or_insert_cached_value!(
                self.cache,
                CacheValue::Tile,
                s.get_tile(xyz, self.query_obj.as_ref()),
                {
                    let id = s.get_id().to_owned();
                    if let Some(query_str) = self.query_str {
                        CacheKey::TileWithQuery(id, xyz, query_str.to_owned())
                    } else {
                        CacheKey::Tile(id, xyz)
                    }
                }
            )
        }))
        .await
        .map_err(map_internal_error)?;

        let mut layer_count = 0;
        let mut last_non_empty_layer = 0;
        for (idx, tile) in tiles.iter().enumerate() {
            if !tile.is_empty() {
                layer_count += 1;
                last_non_empty_layer = idx;
            }
        }

        // Minor optimization to prevent concatenation if there are less than 2 tiles
        let data = match layer_count {
            1 => tiles.swap_remove(last_non_empty_layer),
            0 => return Ok(Tile::new(Vec::new(), self.info)),
            _ => {
                // Make sure tiles can be concatenated, or if not, that there is only one non-empty tile for each zoom level
                // TODO: can zlib, brotli, or zstd be concatenated?
                // TODO: implement decompression step for other concatenate-able formats
                let can_join = self.info.format == Format::Mvt
                    && (self.info.encoding == Encoding::Uncompressed
                        || self.info.encoding == Encoding::Gzip);
                if !can_join {
                    return Err(ErrorBadRequest(format!(
                        "Can't merge {} tiles. Make sure there is only one non-empty tile source at zoom level {}",
                        self.info,
                        xyz.z
                    )))?;
                }
                tiles.concat()
            }
        };

        // decide if (re-)encoding of the tile data is needed, and recompress if so
        self.recompress(data)
    }

    fn recompress(&self, tile: TileData) -> ActixResult<Tile> {
        let mut tile = Tile::new(tile, self.info);
        if let Some(accept_enc) = &self.accept_encodings {
            if self.info.encoding.is_encoded() {
                // already compressed, see if we can send it as is, or need to re-compress
                if !accept_enc.iter().any(|e| {
                    if let Preference::Specific(HeaderEnc::Known(enc)) = e.item {
                        to_encoding(enc) == Some(tile.info.encoding)
                    } else {
                        false
                    }
                }) {
                    // need to re-compress the tile - uncompress it first
                    tile = decode(tile)?;
                }
            }
            if tile.info.encoding == Encoding::Uncompressed {
                let ordered_encodings = if let Some(prefered) = self.prefered_encodings {
                    match prefered {
                        PreferredEncoding::Brotli => [
                            HeaderEnc::brotli(),
                            HeaderEnc::gzip(),
                            HeaderEnc::identity(),
                        ],
                        PreferredEncoding::Gzip => [
                            HeaderEnc::gzip(),
                            HeaderEnc::brotli(),
                            HeaderEnc::identity(),
                        ],
                    }
                } else {
                    [
                        HeaderEnc::brotli(),
                        HeaderEnc::gzip(),
                        HeaderEnc::identity(),
                    ]
                };
                // only apply compression if the content supports it
                if let Some(HeaderEnc::Known(enc)) =
                    // accept_enc.negotiate(SUPPORTED_ENCODINGS.iter())
                    accept_enc.negotiate(ordered_encodings.iter())
                {
                    // (re-)compress the tile into the preferred encoding
                    tile = encode(tile, enc)?;
                }
            }
            Ok(tile)
        } else {
            // no accepted-encoding header, decode the tile if compressed
            decode(tile)
        }
    }
}

fn encode(tile: Tile, enc: ContentEncoding) -> ActixResult<Tile> {
    Ok(match enc {
        ContentEncoding::Brotli => Tile::new(
            encode_brotli(&tile.data)?,
            tile.info.encoding(Encoding::Brotli),
        ),
        ContentEncoding::Gzip => {
            Tile::new(encode_gzip(&tile.data)?, tile.info.encoding(Encoding::Gzip))
        }
        _ => tile,
    })
}

fn decode(tile: Tile) -> ActixResult<Tile> {
    let info = tile.info;
    Ok(if info.encoding.is_encoded() {
        match info.encoding {
            Encoding::Gzip => Tile::new(
                decode_gzip(&tile.data)?,
                info.encoding(Encoding::Uncompressed),
            ),
            Encoding::Brotli => Tile::new(
                decode_brotli(&tile.data)?,
                info.encoding(Encoding::Uncompressed),
            ),
            _ => Err(ErrorBadRequest(format!(
                "Tile is is stored as {info}, but the client does not accept this encoding"
            )))?,
        }
    } else {
        tile
    })
}

pub fn to_encoding(val: ContentEncoding) -> Option<Encoding> {
    Some(match val {
        ContentEncoding::Identity => Encoding::Uncompressed,
        ContentEncoding::Gzip => Encoding::Gzip,
        ContentEncoding::Brotli => Encoding::Brotli,
        // TODO: Deflate => Encoding::Zstd or Encoding::Zlib ?
        _ => None?,
    })
}

#[cfg(test)]
mod tests {
    use actix_http::header::HeaderValue;
    use tilejson::tilejson;

    use super::*;
    use crate::srv::server::tests::TestSource;

    #[actix_rt::test]
    async fn test_encoding_preference() {
        let source = TestSource {
            id: "test_source",
            tj: tilejson! { tiles: vec![] },
            data: vec![1_u8, 2, 3],
        };
        let sources = TileSources::new(vec![vec![Box::new(source)]]);

        for (accept_encodings, prefered_encoding, result_encoding) in [
            (
                Some(AcceptEncoding(vec![
                    "gzip;q=1".parse().unwrap(),
                    "br;q=1".parse().unwrap(),
                ])),
                Some(PreferredEncoding::Brotli),
                Encoding::Brotli,
            ),
            (
                Some(AcceptEncoding(vec![
                    "gzip;q=1".parse().unwrap(),
                    "br;q=0.5".parse().unwrap(),
                ])),
                Some(PreferredEncoding::Brotli),
                Encoding::Gzip,
            ),
        ] {
            let src = DynTileSource::new(
                &sources,
                "test_source",
                None,
                "",
                accept_encodings,
                prefered_encoding,
                None,
            )
            .unwrap();
            let xyz = TileCoord { z: 0, x: 0, y: 0 };
            let data = &src.get_tile_content(xyz).await.unwrap().data;
            let decoded = match result_encoding {
                Encoding::Gzip => decode_gzip(data),
                Encoding::Brotli => decode_brotli(data),
                _ => panic!("Unexpected encoding"),
            };
            assert_eq!(vec![1_u8, 2, 3], decoded.unwrap());
        }
    }
    #[actix_rt::test]
    async fn test_tile_content() {
        let non_empty_source = TestSource {
            id: "non-empty",
            tj: tilejson! { tiles: vec![] },
            data: vec![1_u8, 2, 3],
        };
        let empty_source = TestSource {
            id: "empty",
            tj: tilejson! { tiles: vec![] },
            data: Vec::default(),
        };
        let sources = TileSources::new(vec![vec![
            Box::new(non_empty_source),
            Box::new(empty_source),
        ]]);

        for (source_id, expected) in &[
            ("non-empty", vec![1_u8, 2, 3]),
            ("empty", Vec::<u8>::new()),
            ("empty,empty", Vec::<u8>::new()),
            ("non-empty,non-empty", vec![1_u8, 2, 3, 1_u8, 2, 3]),
            ("non-empty,empty", vec![1_u8, 2, 3]),
            ("non-empty,empty,non-empty", vec![1_u8, 2, 3, 1_u8, 2, 3]),
            ("empty,non-empty", vec![1_u8, 2, 3]),
            ("empty,non-empty,empty", vec![1_u8, 2, 3]),
        ] {
            let src = DynTileSource::new(&sources, source_id, None, "", None, None, None).unwrap();
            let xyz = TileCoord { z: 0, x: 0, y: 0 };
            assert_eq!(expected, &src.get_tile_content(xyz).await.unwrap().data);
        }
    }
}
