use actix_middleware_etag::Etag;
use actix_web::error::ErrorInternalServerError;
use actix_web::http::header::{
    AcceptEncoding, CONTENT_ENCODING, ContentEncoding, Encoding as HeaderEnc, LOCATION, VARY,
};
use actix_web::web::{Bytes, Data, Path};
use actix_web::{
    HttpMessage as _, HttpRequest, HttpResponse, Result as ActixResult, route, routes,
};
use martin_core::cache::CacheKey as _;
use martin_core::fonts::{FontCacheKey, FontError, FontSources, OptFontCache, normalize_font_ids};
use martin_tile_utils::{
    Encoding, encode_brotli_with_quality, encode_gzip, encode_zlib, encode_zstd,
};
use serde::Deserialize;
use tracing::{instrument, warn};

use crate::srv::server::{DebouncedWarning, map_error};

#[derive(Deserialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "unstable-schemas", into_params(parameter_in = Path))]
struct FontRequest {
    fontstack: String,
    start: u32,
    end: u32,
}

#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/font/{fontstack}/{start}-{end}",
        params(FontRequest),
        responses(
            (status = 200, description = "Glyph PBF range", content_type = "application/x-protobuf"),
            (status = 400, description = "Invalid glyph range"),
            (status = 404, description = "No matching font"),
        ),
    )
)]
#[route(
    "/font/{fontstack}/{start}-{end}",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()"
)]
#[hotpath::measure]
#[instrument(
    level = "debug",
    skip_all,
    fields(
        font.fontstack = %path.fontstack,
        font.range.start = path.start,
        font.range.end = path.end,
    ),
    err(Debug),
)]
pub async fn get_font(
    req: HttpRequest,
    path: Path<FontRequest>,
    fonts: Data<FontSources>,
    cache: Data<OptFontCache>,
) -> ActixResult<HttpResponse> {
    let Some(encoding) = negotiate_encoding(req.get_header::<AcceptEncoding>()) else {
        return Ok(HttpResponse::NotAcceptable()
            .insert_header((VARY, "accept-encoding"))
            .body("br, gzip, deflate, zstd"));
    };
    let data = if let Some(cache) = cache.as_ref() {
        // Key the cache by the fonts the request resolves to, not by the alias,
        // so invalidating a font also evicts entries reached through an alias.
        let expanded_ids = fonts.expand_font_ids(&path.fontstack);
        let key = FontCacheKey::new(normalize_font_ids(&expanded_ids), path.start, path.end);
        let served =
            (encoding != Encoding::Uncompressed).then(|| key.clone().with_encoding(encoding));
        if let Some(served) = &served
            && let Some(data) = cache.get(served).await
        {
            served.record_outcome(true);
            data
        } else {
            let glyphs = cache
                .get_or_insert(key, async || {
                    fonts
                        .get_font_range(&path.fontstack, path.start, path.end)
                        .map(Bytes::from)
                })
                .await
                .map_err(|e| map_font_error(e.as_ref()))?;
            let data = compress(glyphs, encoding)?;
            if let Some(served) = served {
                cache.insert(served, data.clone()).await;
            }
            data
        }
    } else {
        let glyphs = fonts
            .get_font_range(&path.fontstack, path.start, path.end)
            .map_err(|e| map_font_error(&e))?;
        compress(glyphs.into(), encoding)?
    };
    let mut response = HttpResponse::Ok();
    response
        .content_type("application/x-protobuf")
        .insert_header((VARY, "accept-encoding"));
    if let Some(coding) = encoding.compression() {
        response.insert_header((CONTENT_ENCODING, coding));
    }
    Ok(response.body(data))
}

/// The content codings a glyph range can be served in.
const SUPPORTED_ENCODINGS: &[HeaderEnc] = &[
    HeaderEnc::identity(),
    HeaderEnc::brotli(),
    HeaderEnc::gzip(),
    HeaderEnc::deflate(),
    HeaderEnc::zstd(),
];

/// Brotli quality for compressed glyph ranges.
const BROTLI_QUALITY: u32 = 6;

/// The encoding a glyph range is served in, `None` when the client accepts none of them.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "actix's ContentEncoding is #[non_exhaustive]; identity and unknown codings are served as-is"
)]
fn negotiate_encoding(accept: Option<AcceptEncoding>) -> Option<Encoding> {
    let Some(accept) = accept else {
        return Some(Encoding::Uncompressed);
    };
    Some(match accept.negotiate(SUPPORTED_ENCODINGS.iter())? {
        HeaderEnc::Known(ContentEncoding::Brotli) => Encoding::Brotli,
        HeaderEnc::Known(ContentEncoding::Gzip) => Encoding::Gzip,
        HeaderEnc::Known(ContentEncoding::Deflate) => Encoding::Zlib,
        HeaderEnc::Known(ContentEncoding::Zstd) => Encoding::Zstd,
        _ => Encoding::Uncompressed,
    })
}

/// Compresses a glyph range into `encoding`.
fn compress(glyphs: Bytes, encoding: Encoding) -> ActixResult<Bytes> {
    let compressed = match encoding {
        Encoding::Gzip => encode_gzip(&glyphs),
        Encoding::Zlib => encode_zlib(&glyphs),
        Encoding::Brotli => encode_brotli_with_quality(&glyphs, BROTLI_QUALITY),
        Encoding::Zstd => encode_zstd(&glyphs),
        Encoding::Uncompressed | Encoding::Internal => return Ok(glyphs),
    };
    compressed
        .map(Bytes::from)
        .map_err(ErrorInternalServerError)
}

/// Redirect `/fonts/{fontstack}/{start}-{end}` to `/font/{fontstack}/{start}-{end}` (HTTP 301)
#[route("/fonts/{fontstack}/{start}-{end}", method = "GET", method = "HEAD")]
pub async fn redirect_fonts(path: Path<FontRequest>) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();

    WARNING
        .once_per_hour(|| {
            warn!(
                "Request to /fonts/{}/{}-{} caused unnecessary redirect. Use /font/{}/{}-{} to avoid extra round-trip latency.",
                path.fontstack, path.start, path.end, path.fontstack, path.start, path.end
            );
        })
        .await;

    HttpResponse::MovedPermanently()
        .insert_header((
            LOCATION,
            format!("/font/{}/{}-{}", path.fontstack, path.start, path.end),
        ))
        .finish()
}

#[derive(Deserialize, Debug)]
struct FontExtRequest {
    fontstack: String,
    start: u32,
    end: u32,
    ext: String,
}

/// Redirect `/font/{fontstack}/{start}-{end}.{extension}` to `/font/{fontstack}/{start}-{end}` (HTTP 301)
#[routes]
#[get("/font/{fontstack}/{start}-{end}.{ext}")]
#[head("/font/{fontstack}/{start}-{end}.{ext}")]
#[get("/fonts/{fontstack}/{start}-{end}.{ext}")]
#[head("/fonts/{fontstack}/{start}-{end}.{ext}")]
pub async fn redirect_font_ext(path: Path<FontExtRequest>) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();
    let FontExtRequest {
        fontstack,
        start,
        end,
        ext,
    } = path.as_ref();

    WARNING
        .once_per_hour(|| {
            warn!(
                "Request to /font/{fontstack}/{start}-{end}.{ext} caused unnecessary redirect. Use /font/{fontstack}/{start}-{end} to avoid extra round-trip latency."
            );
        })
        .await;

    HttpResponse::MovedPermanently()
        .insert_header((LOCATION, format!("/font/{fontstack}/{start}-{end}")))
        .finish()
}

pub fn map_font_error(e: &FontError) -> actix_web::Error {
    map_error(e)
}
