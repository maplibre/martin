use std::sync::Arc;

use actix_http::ContentEncoding;
use actix_http::header::Quality;
use actix_web::error::{ErrorBadRequest, ErrorNotAcceptable, ErrorNotFound};
use actix_web::http::header::{
    AcceptEncoding, CONTENT_ENCODING, ETAG, Encoding as HeaderEnc, EntityTag, IfNoneMatch,
    LOCATION, Preference,
};
use actix_web::web::{Data, Path, Query};
use actix_web::{HttpMessage as _, HttpRequest, HttpResponse, Result as ActixResult, route};
use futures::future::try_join_all;
use martin_core::tiles::{BoxedSource, Tile, TileCache, UrlQuery};
use martin_tile_utils::{
    Encoding, Format, TileCoord, TileData, TileInfo, decode_brotli, decode_gzip, decode_zlib,
    decode_zstd, encode_brotli, encode_gzip, encode_zlib, encode_zstd,
};
use serde::Deserialize;
use tracing::warn;

use crate::config::args::PreferredEncoding;
use crate::config::file::srv::SrvConfig;
use crate::srv::server::{DebouncedWarning, map_internal_error};
use crate::tile_source_manager::TileSourceManager;

const SUPPORTED_ENC: &[HeaderEnc] = &[
    HeaderEnc::gzip(),
    HeaderEnc::brotli(),
    HeaderEnc::zstd(),
    //probably dont need deflate here, most clients don't support
    HeaderEnc::identity(),
];

#[derive(Deserialize, Clone)]
pub struct TileRequest {
    source_ids: String,
    z: u8,
    x: u32,
    y: u32,
}

#[route("/{source_ids}/{z}/{x}/{y}", method = "GET", method = "HEAD")]
#[hotpath::measure]
async fn get_tile(
    req: HttpRequest,
    srv_config: Data<SrvConfig>,
    path: Path<TileRequest>,
    manager: Data<TileSourceManager>,
) -> ActixResult<HttpResponse> {
    let src = DynTileSource::new(
        &manager,
        &path.source_ids,
        Some(path.z),
        req.query_string(),
        req.get_header::<AcceptEncoding>(),
        req.get_header::<IfNoneMatch>(),
        srv_config.preferred_encoding,
    )?;

    src.get_http_response(TileCoord {
        z: path.z,
        x: path.x,
        y: path.y,
    })
    .await
}

#[derive(Deserialize, Clone)]
pub struct RedirectTileRequest {
    ids: String,
    z: u8,
    x: u32,
    y: u32,
    ext: String,
}

/// Redirect `/{source_ids}/{z}/{x}/{y}.{extension}` to `/{source_ids}/{z}/{x}/{y}` (HTTP 301)
/// Registered before main tile route to match more specific pattern first
#[route("/{ids}/{z}/{x}/{y}.{ext}", method = "GET", method = "HEAD")]
pub async fn redirect_tile_ext(
    req: HttpRequest,
    path: Path<RedirectTileRequest>,
    srv_config: Data<SrvConfig>,
) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();
    let RedirectTileRequest { ids, z, x, y, ext } = path.as_ref();

    WARNING
        .once_per_hour(|| {
            warn!(
                "Request to /{ids}/{z}/{x}/{y}.{ext} caused unnecessary redirect. Use /{ids}/{z}/{x}/{y} to avoid extra round-trip latency."
            );
        })
        .await;

    redirect_tile_with_query(
        ids,
        *z,
        *x,
        *y,
        req.query_string(),
        srv_config.route_prefix.as_deref(),
    )
}

/// Redirect `/tiles/{source_ids}/{z}/{x}/{y}` to `/{source_ids}/{z}/{x}/{y}` (HTTP 301)
#[route("/tiles/{source_ids}/{z}/{x}/{y}", method = "GET", method = "HEAD")]
pub async fn redirect_tiles(
    req: HttpRequest,
    path: Path<TileRequest>,
    srv_config: Data<SrvConfig>,
) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();
    let TileRequest {
        source_ids,
        z,
        x,
        y,
    } = path.as_ref();

    WARNING
        .once_per_hour(|| {
            warn!(
                "Request to /tiles/{source_ids}/{z}/{x}/{y} caused unnecessary redirect. Use /{source_ids}/{z}/{x}/{y} to avoid extra round-trip latency."
            );
        })
        .await;

    redirect_tile_with_query(
        source_ids,
        *z,
        *x,
        *y,
        req.query_string(),
        srv_config.route_prefix.as_deref(),
    )
}

/// Helper function to create a 301 redirect for tiles with query string preservation
fn redirect_tile_with_query(
    source_ids: &str,
    z: u8,
    x: u32,
    y: u32,
    query_string: &str,
    route_prefix: Option<&str>,
) -> HttpResponse {
    let location = if let Some(prefix) = route_prefix {
        format!("{prefix}/{source_ids}/{z}/{x}/{y}")
    } else {
        format!("/{source_ids}/{z}/{x}/{y}")
    };
    let location = if query_string.is_empty() {
        location
    } else {
        format!("{location}?{query_string}")
    };
    HttpResponse::MovedPermanently()
        .insert_header((LOCATION, location))
        .finish()
}

pub struct DynTileSource<'a> {
    pub sources: Vec<BoxedSource>,
    pub info: TileInfo,
    pub query_str: Option<&'a str>,
    pub query_obj: Option<UrlQuery>,
    pub accept_enc: Option<AcceptEncoding>,
    pub if_none_match: Option<IfNoneMatch>,
    pub preferred_enc: Option<PreferredEncoding>,
    pub cache: Option<&'a TileCache>,
}

impl<'a> DynTileSource<'a> {
    #[hotpath::measure]
    pub fn new(
        manager: &'a TileSourceManager,
        source_ids: &str,
        zoom: Option<u8>,
        query: &'a str,
        accept_enc: Option<AcceptEncoding>,
        if_none_match: Option<IfNoneMatch>,
        preferred_enc: Option<PreferredEncoding>,
    ) -> ActixResult<Self> {
        let tile_sources = manager.tile_sources();
        let (sources, use_url_query, info) = tile_sources.get_sources(source_ids, zoom)?;
        let cache = manager.tile_cache().as_ref();

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
            accept_enc,
            if_none_match,
            preferred_enc,
            cache,
        })
    }

    #[hotpath::measure]
    pub async fn get_http_response(&self, xyz: TileCoord) -> ActixResult<HttpResponse> {
        let tile = self.get_tile_content(xyz).await?;
        if tile.data.is_empty() {
            return Ok(HttpResponse::NoContent().finish());
        }
        let etag = EntityTag::new_strong(tile.etag.clone());

        if let Some(IfNoneMatch::Items(expected_etags)) = &self.if_none_match {
            for expected_etag in expected_etags {
                if etag.strong_eq(expected_etag) {
                    return Ok(HttpResponse::NotModified().finish());
                }
            }
        }

        let mut response = HttpResponse::Ok();
        response.content_type(tile.info.format.content_type());
        response.insert_header((ETAG, etag));
        if let Some(val) = tile.info.encoding.content_encoding() {
            response.insert_header((CONTENT_ENCODING, val));
        }
        Ok(response.body(tile.data))
    }

    #[hotpath::measure]
    pub async fn get_tile_content(&self, xyz: TileCoord) -> ActixResult<Tile> {
        let mut tiles = try_join_all(self.sources.iter().map(|s| async {
            let cache_zoom_ok = s.cache_zoom().contains(xyz.z);
            if let (Some(cache), true) = (self.cache, cache_zoom_ok) {
                cache
                    .get_or_insert(
                        s.get_id().to_string(),
                        xyz,
                        self.query_str.map(ToString::to_string),
                        || s.get_tile_with_etag(xyz, self.query_obj.as_ref()),
                    )
                    .await
            } else {
                s.get_tile_with_etag(xyz, self.query_obj.as_ref())
                    .await
                    .map_err(Arc::new)
            }
        }))
        .await
        .map_err(|e| map_internal_error(e.as_ref()))?;

        let mut layer_count = 0;
        let mut last_non_empty_layer = 0;
        for (idx, tile) in tiles.iter().enumerate() {
            if !tile.is_empty() {
                layer_count += 1;
                last_non_empty_layer = idx;
            }
        }

        // Minor optimization to prevent concatenation if there are less than 2 tiles
        let (data, etag, effective_info) = match layer_count {
            0 => return Ok(Tile::new_hash_etag(Vec::new(), self.info)),
            1 => {
                let tile = tiles.swap_remove(last_non_empty_layer);
                (tile.data, tile.etag, tile.info)
            }
            _ => {
                let can_join = (self.info.format == Format::Mvt || self.info.format == Format::Mlt)
                    && tiles.iter().all(|t| t.info.format == self.info.format);
                if !can_join {
                    return Err(ErrorBadRequest(format!(
                        "Cannot merge non-MVT formats. Format is {:?} with encoding {:?} ",
                        self.info.format, self.info.encoding,
                    )));
                }

                // Build combined etag before consuming tiles
                let total_etag_len: usize = tiles.iter().map(|t| t.etag.len()).sum();
                let mut combined_etag = String::with_capacity(total_etag_len);
                for tile in &tiles {
                    combined_etag.push_str(&tile.etag);
                }

                let (concat_data, effective_info) = if self.info.encoding == Encoding::Uncompressed
                    || self.info.encoding == Encoding::Gzip
                {
                    // Gzip multi-stream is valid; uncompressed concat is fine
                    let data = tiles
                        .into_iter()
                        .map(|t| t.data)
                        .collect::<Vec<_>>()
                        .concat();
                    (data, self.info)
                } else {
                    // Decompress first, concat raw MVT, let recompress re-encode
                    let mut raw = Vec::new();
                    for tile_data in tiles {
                        let t = Tile::new_with_etag(tile_data.data, tile_data.info, tile_data.etag);
                        let decoded = decode(t)?;
                        raw.extend_from_slice(&decoded.data);
                    }
                    (raw, self.info.encoding(Encoding::Uncompressed))
                };

                (concat_data, combined_etag, effective_info)
            }
        };

        // decide if (re-)encoding of the tile data is needed, and recompress if so
        let mut tile = self.recompress(data, effective_info)?;
        // Set the etag for the final tile
        tile.etag = etag;
        Ok(tile)
    }

    /// Decide which encoding to use for the uncompressed tile data, based on the client's Accept-Encoding header
    fn decide_encoding(&self, accept_enc: &AcceptEncoding) -> ActixResult<Option<ContentEncoding>> {
        let mut q_gzip = None;
        let mut q_brotli = None;
        let mut q_zstd = None;
        for enc in accept_enc.iter() {
            if let Preference::Specific(HeaderEnc::Known(e)) = enc.item {
                match e {
                    ContentEncoding::Gzip => q_gzip = Some(enc.quality),
                    ContentEncoding::Brotli => q_brotli = Some(enc.quality),
                    ContentEncoding::Zstd => q_zstd = Some(enc.quality),
                    _ => {}
                }
            } else if let Preference::Any = enc.item {
                q_gzip.get_or_insert(enc.quality);
                q_brotli.get_or_insert(enc.quality);
                q_zstd.get_or_insert(enc.quality);
            }
        }
        if let (Some(qg), Some(qb)) = (q_gzip, q_brotli) {
            let qz = q_zstd.unwrap_or(Quality::ZERO);
            let max_q = if qg >= qb && qg >= qz {
                qg
            } else if qb >= qz {
                qb
            } else {
                qz
            };
            if max_q == Quality::ZERO {
                return Ok(None);
            }
            let at_max = u8::from(qg == max_q) + u8::from(qb == max_q) + u8::from(qz == max_q);
            return Ok(Some(if at_max > 1 {
                self.get_preferred_enc()
            } else if qb == max_q {
                ContentEncoding::Brotli
            } else if qz == max_q {
                ContentEncoding::Zstd
            } else {
                ContentEncoding::Gzip
            }));
        }
        if let Some(HeaderEnc::Known(enc)) = accept_enc.negotiate(SUPPORTED_ENC.iter()) {
            Ok(Some(enc))
        } else {
            Err(ErrorNotAcceptable("No supported encoding found"))
        }
    }

    fn get_preferred_enc(&self) -> ContentEncoding {
        match self.preferred_enc {
            None | Some(PreferredEncoding::Gzip) => ContentEncoding::Gzip,
            Some(PreferredEncoding::Brotli) => ContentEncoding::Brotli,
        }
    }

    #[hotpath::measure]
    fn recompress(&self, tile: TileData, info: TileInfo) -> ActixResult<Tile> {
        let mut tile = Tile::new_hash_etag(tile, info);
        if let Some(accept_enc) = &self.accept_enc {
            if info.encoding.is_encoded() {
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

            if tile.info.encoding == Encoding::Uncompressed
                && let Some(enc) = self.decide_encoding(accept_enc)?
            {
                // (re-)compress the tile into the preferred encoding
                tile = encode(tile, enc)?;
            }
            Ok(tile)
        } else {
            // no accepted-encoding header, decode the tile if compressed
            decode(tile)
        }
    }
}

#[hotpath::measure]
fn encode(tile: Tile, enc: ContentEncoding) -> ActixResult<Tile> {
    hotpath::dbg!("encode", enc);
    Ok(match enc {
        ContentEncoding::Brotli => Tile::new_hash_etag(
            encode_brotli(&tile.data)?,
            tile.info.encoding(Encoding::Brotli),
        ),
        ContentEncoding::Gzip => {
            Tile::new_hash_etag(encode_gzip(&tile.data)?, tile.info.encoding(Encoding::Gzip))
        }
        ContentEncoding::Deflate => {
            Tile::new_hash_etag(encode_zlib(&tile.data)?, tile.info.encoding(Encoding::Zlib))
        }
        ContentEncoding::Zstd => {
            Tile::new_hash_etag(encode_zstd(&tile.data)?, tile.info.encoding(Encoding::Zstd))
        }
        _ => tile,
    })
}

#[hotpath::measure]
fn decode(tile: Tile) -> ActixResult<Tile> {
    let info = tile.info;
    Ok(if info.encoding.is_encoded() {
        match info.encoding {
            Encoding::Gzip => Tile::new_hash_etag(
                decode_gzip(&tile.data)?,
                info.encoding(Encoding::Uncompressed),
            ),
            Encoding::Brotli => Tile::new_hash_etag(
                decode_brotli(&tile.data)?,
                info.encoding(Encoding::Uncompressed),
            ),
            Encoding::Zlib => Tile::new_hash_etag(
                decode_zlib(&tile.data)?,
                info.encoding(Encoding::Uncompressed),
            ),
            Encoding::Zstd => Tile::new_hash_etag(
                decode_zstd(&tile.data)?,
                info.encoding(Encoding::Uncompressed),
            ),
            _ => Err(ErrorBadRequest(format!(
                "Tile is stored as {info}, but the client does not accept this encoding"
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
        ContentEncoding::Deflate => Encoding::Zlib,
        ContentEncoding::Zstd => Encoding::Zstd,
        _ => None?,
    })
}

#[cfg(test)]
mod tests {
    use actix_http::header::TryIntoHeaderValue as _;
    use rstest::rstest;
    use tilejson::tilejson;

    use super::*;
    use crate::config::file::OnInvalid;
    use crate::srv::tiles::tests::{CompressedTestSource, TestSource};

    fn test_manager(sources: Vec<Vec<BoxedSource>>) -> TileSourceManager {
        TileSourceManager::from_sources(None, OnInvalid::Abort, sources)
    }

    #[rstest]
    #[trace]
    #[case(&["gzip", "deflate", "br", "zstd"], None, Encoding::Gzip)]
    #[case(&["gzip", "deflate", "br", "zstd"], Some(PreferredEncoding::Brotli), Encoding::Brotli)]
    #[case(&["gzip", "deflate", "br", "zstd"], Some(PreferredEncoding::Gzip), Encoding::Gzip)]
    #[case(&["br;q=1", "gzip;q=1"], Some(PreferredEncoding::Gzip), Encoding::Gzip)]
    #[case(&["gzip;q=1", "br;q=1"], Some(PreferredEncoding::Brotli), Encoding::Brotli)]
    #[case(&["gzip;q=1", "br;q=0.5"], Some(PreferredEncoding::Brotli), Encoding::Gzip)]
    #[case(&["gzip;q=0.5", "br;q=0.5", "zstd;q=1.0"], None, Encoding::Zstd)]
    #[case(&["gzip;q=0.5", "br;q=0.5", "zstd;q=1.0"], Some(PreferredEncoding::Brotli), Encoding::Zstd)]
    #[actix_rt::test]
    async fn test_enc_preference(
        #[case] accept_enc: &[&'static str],
        #[case] preferred_enc: Option<PreferredEncoding>,
        #[case] expected_enc: Encoding,
    ) {
        let mgr = test_manager(vec![vec![Box::new(TestSource {
            id: "test_source",
            tj: tilejson! { tiles: vec![] },
            data: vec![1_u8, 2, 3],
        })]]);

        let accept_enc = Some(AcceptEncoding(
            accept_enc.iter().map(|s| s.parse().unwrap()).collect(),
        ));

        let src = DynTileSource::new(
            &mgr,
            "test_source",
            None,
            "",
            accept_enc,
            None,
            preferred_enc,
        )
        .unwrap();

        let xyz = TileCoord { z: 0, x: 0, y: 0 };
        let tile = src.get_tile_content(xyz).await.unwrap();
        assert_eq!(tile.info.encoding, expected_enc);
    }

    #[rstest]
    #[case(200, None, Some(EntityTag::new_strong("O3OuMnabzuvUuMTLiOt3rA".to_string())))]
    #[case(304, Some(IfNoneMatch::Items(vec![EntityTag::new_strong("O3OuMnabzuvUuMTLiOt3rA".to_string())])), None)]
    #[case(200, Some(IfNoneMatch::Items(vec![EntityTag::new_strong("incorrect_etag".to_string())])), Some(EntityTag::new_strong("O3OuMnabzuvUuMTLiOt3rA".to_string())))]
    #[actix_rt::test]
    async fn test_etag(
        #[case] expected_status: u16,
        #[case] if_none_match: Option<IfNoneMatch>,
        #[case] expected_etag: Option<EntityTag>,
    ) {
        let source_id = "source1";
        let source1 = TestSource {
            id: source_id,
            tj: tilejson! { tiles: vec![] },
            data: vec![1_u8, 2, 3],
        };
        let mgr = test_manager(vec![vec![Box::new(source1)]]);

        let src = DynTileSource::new(&mgr, source_id, None, "", None, if_none_match, None).unwrap();
        let resp = &src
            .get_http_response(TileCoord { z: 0, x: 0, y: 0 })
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), expected_status);
        let etag = resp.headers().get(ETAG);
        assert_eq!(
            etag,
            expected_etag.map(|e| e.try_into_value().unwrap()).as_ref()
        );
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
        let mgr = test_manager(vec![vec![
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
            let src = DynTileSource::new(&mgr, source_id, None, "", None, None, None).unwrap();
            let xyz = TileCoord { z: 0, x: 0, y: 0 };
            assert_eq!(expected, &src.get_tile_content(xyz).await.unwrap().data);
        }
    }

    fn compress_with(data: &[u8], encoding: Encoding) -> Vec<u8> {
        match encoding {
            Encoding::Brotli => encode_brotli(data).unwrap(),
            Encoding::Zlib => encode_zlib(data).unwrap(),
            Encoding::Zstd => encode_zstd(data).unwrap(),
            _ => panic!("compress_with: unsupported encoding {encoding:?}"),
        }
    }

    fn decompress_tile(data: &[u8], encoding: Encoding) -> Vec<u8> {
        match encoding {
            Encoding::Uncompressed => data.to_vec(),
            Encoding::Gzip => decode_gzip(data).unwrap(),
            Encoding::Brotli => decode_brotli(data).unwrap(),
            Encoding::Zlib => decode_zlib(data).unwrap(),
            Encoding::Zstd => decode_zstd(data).unwrap(),
            Encoding::Internal => {
                panic!("decompress_tile: cannot decompress tile with internal encoding")
            }
        }
    }

    #[rstest]
    #[case(Encoding::Brotli, None, Encoding::Uncompressed)]
    #[case(Encoding::Zlib, None, Encoding::Uncompressed)]
    #[case(Encoding::Zstd, None, Encoding::Uncompressed)]
    #[case(Encoding::Brotli, Some("zstd"), Encoding::Zstd)]
    #[case(Encoding::Zlib, Some("br"), Encoding::Brotli)]
    #[case(Encoding::Zstd, Some("gzip"), Encoding::Gzip)]
    #[actix_rt::test]
    async fn test_compressed_mvt_merge(
        #[case] src_enc: Encoding,
        #[case] accept: Option<&str>,
        #[case] expected_enc: Encoding,
    ) {
        let raw1: Vec<u8> = vec![1, 2, 3];
        let raw2: Vec<u8> = vec![4, 5, 6];

        let src1 = CompressedTestSource {
            id: "src1",
            tj: tilejson! { tiles: vec![] },
            data: compress_with(&raw1, src_enc),
            encoding: src_enc,
        };
        let src2 = CompressedTestSource {
            id: "src2",
            tj: tilejson! { tiles: vec![] },
            data: compress_with(&raw2, src_enc),
            encoding: src_enc,
        };

        let mgr = test_manager(vec![vec![Box::new(src1), Box::new(src2)]]);

        let accept_enc = accept.map(|s| AcceptEncoding(vec![s.parse().unwrap()]));
        let src = DynTileSource::new(&mgr, "src1,src2", None, "", accept_enc, None, None).unwrap();

        let tile = src
            .get_tile_content(TileCoord { z: 0, x: 0, y: 0 })
            .await
            .unwrap();

        assert_eq!(
            tile.info.encoding, expected_enc,
            "wrong output encoding for src={src_enc:?}, accept={accept:?}"
        );

        let decoded = decompress_tile(&tile.data, tile.info.encoding);
        let expected_raw: Vec<u8> = raw1.iter().chain(raw2.iter()).copied().collect();
        assert_eq!(
            decoded, expected_raw,
            "decoded content mismatch for src={src_enc:?}, accept={accept:?}"
        );
    }
}
