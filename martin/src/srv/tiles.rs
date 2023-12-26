use actix_http::ContentEncoding;
use actix_web::error::{ErrorBadRequest, ErrorNotFound};
use actix_web::http::header::{
    AcceptEncoding, Encoding as HeaderEnc, Preference, CONTENT_ENCODING,
};
use actix_web::web::{Data, Path, Query};
use actix_web::{route, HttpMessage, HttpRequest, HttpResponse, Result as ActixResult};
use futures::future::try_join_all;
use martin_tile_utils::{Encoding, Format, TileInfo};
use serde::Deserialize;

use crate::source::{Source, TileSources, UrlQuery};
use crate::srv::server::map_internal_error;
use crate::utils::{decode_brotli, decode_gzip, encode_brotli, encode_gzip};
use crate::{Tile, TileCoord};

static SUPPORTED_ENCODINGS: &[HeaderEnc] = &[
    HeaderEnc::brotli(),
    HeaderEnc::gzip(),
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
async fn get_tile(
    req: HttpRequest,
    path: Path<TileRequest>,
    sources: Data<TileSources>,
) -> ActixResult<HttpResponse> {
    let xyz = TileCoord {
        z: path.z,
        x: path.x,
        y: path.y,
    };

    let source_ids = &path.source_ids;
    let query = req.query_string();
    let encodings = req.get_header::<AcceptEncoding>();

    get_tile_response(sources.as_ref(), xyz, source_ids, query, encodings).await
}

pub async fn get_tile_response(
    sources: &TileSources,
    xyz: TileCoord,
    source_ids: &str,
    query: &str,
    encodings: Option<AcceptEncoding>,
) -> ActixResult<HttpResponse> {
    let (sources, use_url_query, info) = sources.get_sources(source_ids, Some(xyz.z))?;

    let query = use_url_query.then_some(query);
    let tile = get_tile_content(sources.as_slice(), info, xyz, query, encodings.as_ref()).await?;

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

pub async fn get_tile_content(
    sources: &[&dyn Source],
    info: TileInfo,
    xyz: TileCoord,
    query: Option<&str>,
    encodings: Option<&AcceptEncoding>,
) -> ActixResult<Tile> {
    if sources.is_empty() {
        return Err(ErrorNotFound("No valid sources found"));
    }
    let query_str = query.filter(|v| !v.is_empty());
    let query = match query_str {
        Some(v) => Some(Query::<UrlQuery>::from_query(v)?.into_inner()),
        None => None,
    };

    let mut tiles = try_join_all(sources.iter().map(|s| s.get_tile(xyz, query.as_ref())))
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
        0 => return Ok(Tile::new(Vec::new(), info)),
        _ => {
            // Make sure tiles can be concatenated, or if not, that there is only one non-empty tile for each zoom level
            // TODO: can zlib, brotli, or zstd be concatenated?
            // TODO: implement decompression step for other concatenate-able formats
            let can_join = info.format == Format::Mvt
                && (info.encoding == Encoding::Uncompressed || info.encoding == Encoding::Gzip);
            if !can_join {
                return Err(ErrorBadRequest(format!(
                    "Can't merge {info} tiles. Make sure there is only one non-empty tile source at zoom level {}",
                    xyz.z
                )))?;
            }
            tiles.concat()
        }
    };

    // decide if (re-)encoding of the tile data is needed, and recompress if so
    let tile = recompress(Tile::new(data, info), encodings)?;

    Ok(tile)
}

fn recompress(mut tile: Tile, accept_enc: Option<&AcceptEncoding>) -> ActixResult<Tile> {
    if let Some(accept_enc) = accept_enc {
        if tile.info.encoding.is_encoded() {
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
            // only apply compression if the content supports it
            if let Some(HeaderEnc::Known(enc)) = accept_enc.negotiate(SUPPORTED_ENCODINGS.iter()) {
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

fn to_encoding(val: ContentEncoding) -> Option<Encoding> {
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
    use tilejson::tilejson;

    use super::*;
    use crate::srv::server::tests::TestSource;

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
            let (src, _, info) = sources.get_sources(source_id, None).unwrap();
            let xyz = TileCoord { z: 0, x: 0, y: 0 };
            assert_eq!(
                expected,
                &get_tile_content(src.as_slice(), info, xyz, None, None)
                    .await
                    .unwrap()
                    .data
            );
        }
    }
}
