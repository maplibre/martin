//! The [`PassthroughSource`] [`Source`] implementation and its HTTP fetch logic.

use std::time::Duration;

use async_trait::async_trait;
use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
use reqwest::StatusCode;
use reqwest::header::{
    CONTENT_ENCODING, CONTENT_TYPE, ETAG, HeaderMap, HeaderName, HeaderValue, USER_AGENT,
};
use tilejson::{Bounds, TileJSON, tilejson};

use crate::CacheZoomRange;
use crate::tiles::passthrough::PassthroughError;
use crate::tiles::passthrough::url::{
    UrlTemplate, derive_format, is_template, select_url, substitute,
};
use crate::tiles::{BoxedSource, MartinCoreError, MartinCoreResult, Source, Tile, UrlQuery};

/// HTTP transport settings applied to both `TileJSON` discovery and per-tile fetches.
#[derive(Clone, Debug)]
pub struct Transport {
    /// Headers sent with every request.
    pub headers: HeaderMap,
    /// Per-request timeout.
    pub timeout: Duration,
}

impl Transport {
    /// A transport with the given per-request timeout and no extra headers.
    #[must_use]
    pub fn new(timeout: Duration) -> Self {
        Self {
            headers: HeaderMap::new(),
            timeout,
        }
    }

    /// Build a transport from string header pairs, validating each name and value.
    ///
    /// This lets callers (e.g. the `martin` crate's config layer) supply headers without
    /// depending on `reqwest`'s header types directly.
    pub fn from_string_headers<'a, I>(
        timeout: Duration,
        headers: I,
    ) -> Result<Self, PassthroughError>
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        let mut header_map = HeaderMap::new();
        for (name, value) in headers {
            let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|source| {
                PassthroughError::InvalidHeaderName {
                    name: name.to_string(),
                    source,
                }
            })?;
            let header_value = HeaderValue::from_str(value).map_err(|source| {
                PassthroughError::InvalidHeaderValue {
                    name: name.to_string(),
                    source,
                }
            })?;
            header_map.insert(header_name, header_value);
        }
        Ok(Self {
            headers: header_map,
            timeout,
        })
    }
}

/// Operator-supplied metadata for a template upstream.
#[derive(Clone, Debug)]
pub struct TemplateMeta {
    /// Minimum zoom level.
    pub minzoom: Option<u8>,
    /// Maximum zoom level.
    pub maxzoom: Option<u8>,
    /// Geographic bounds.
    pub bounds: Option<Bounds>,
    /// Attribution string.
    pub attribution: Option<String>,
}

/// A non-empty set of `{z}/{x}/{y}` templates together with the format and metadata an operator
/// must supply for them.
#[derive(Clone, Debug)]
pub struct TemplateSet {
    urls: Vec<UrlTemplate>,
    format: Format,
    meta: TemplateMeta,
}

impl TemplateSet {
    /// Build a template set, rejecting an empty URL list.
    pub fn new(
        urls: Vec<UrlTemplate>,
        format: Format,
        meta: TemplateMeta,
    ) -> Result<Self, PassthroughError> {
        if urls.is_empty() {
            return Err(PassthroughError::EmptyUrlList);
        }
        Ok(Self { urls, format, meta })
    }

    /// The configured templates, guaranteed non-empty.
    #[must_use]
    pub fn urls(&self) -> &[UrlTemplate] {
        &self.urls
    }

    /// The resolved tile format.
    #[must_use]
    pub fn format(&self) -> Format {
        self.format
    }

    /// The operator-declared metadata.
    #[must_use]
    pub fn meta(&self) -> &TemplateMeta {
        &self.meta
    }
}

/// A classified passthrough upstream.
#[derive(Clone, Debug)]
pub enum Upstream {
    /// One or more `{z}/{x}/{y}` templates plus operator-declared format and metadata.
    Templates(TemplateSet),
    /// A `TileJSON` document URL; tiles, zoom, bounds and attribution come from the document.
    TileJson {
        /// The document URL.
        url: String,
        /// Explicit format override; otherwise derived from the document.
        format: Option<Format>,
    },
}

impl Upstream {
    /// Classify raw config URL strings into a typed upstream.
    ///
    /// A lone non-template URL becomes a [`Upstream::TileJson`] document; otherwise the URLs are
    /// `{z}/{x}/{y}` templates whose `format` is resolved from the override or the first extension.
    /// `meta` applies only to the templates arm.
    pub fn from_config(
        id: &str,
        urls: &[String],
        format: Option<Format>,
        meta: TemplateMeta,
    ) -> Result<Self, PassthroughError> {
        match urls {
            [] => Err(PassthroughError::EmptyUrlList),
            [single] if !is_template(single) => Ok(Self::TileJson {
                url: single.clone(),
                format,
            }),
            raw => {
                let first = raw.first().ok_or(PassthroughError::EmptyUrlList)?;
                let format = derive_format(id, format, first, None)?;
                let templates = raw
                    .iter()
                    .map(|u| UrlTemplate::new(u.clone()))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Self::Templates(TemplateSet::new(templates, format, meta)?))
            }
        }
    }
}

/// A [`Source`] that proxies tiles from an upstream HTTP tile server.
#[derive(Clone, Debug)]
pub struct PassthroughSource {
    id: String,
    client: reqwest::Client,
    /// Resolved `{z}/{x}/{y}` templates (for `TileJSON` sources, taken from `tiles[]`).
    urls: Vec<String>,
    tilejson: TileJSON,
    tile_info: TileInfo,
    cache_zoom: CacheZoomRange,
    /// Kept so [`try_reload`](Source::try_reload) can rebuild (and re-fetch `TileJSON`) from scratch.
    upstream: Upstream,
    /// Kept so [`try_reload`](Source::try_reload) can rebuild the HTTP client.
    transport: Transport,
}

/// A tile fetched from the upstream, before it is wrapped into a [`Tile`].
struct FetchedTile {
    data: TileData,
    info: TileInfo,
    /// The upstream `ETag` header verbatim, if any (so large tiles are not re-hashed).
    etag: Option<String>,
}

impl PassthroughSource {
    /// Build a passthrough source, fetching the upstream `TileJSON` once for a document upstream.
    pub async fn new(
        id: String,
        upstream: Upstream,
        transport: Transport,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, PassthroughError> {
        let client = build_client(&transport)?;

        let (urls, tilejson, tile_info) = match &upstream {
            Upstream::Templates(set) => {
                let urls: Vec<String> = set.urls().iter().map(|t| t.as_str().to_string()).collect();
                let tilejson = build_template_tilejson(&urls, set.meta());
                (urls, tilejson, TileInfo::from(set.format()))
            }
            Upstream::TileJson { url, format } => {
                let upstream_tj = fetch_tilejson(&client, url).await?;
                let templates = upstream_tj.tiles.clone();
                let first = templates
                    .first()
                    .ok_or_else(|| PassthroughError::NoTilesInTileJson(url.clone()))?;
                let tj_format = upstream_tj.other.get("format").and_then(|v| v.as_str());
                let format = derive_format(&id, *format, first, tj_format)?;
                (templates, upstream_tj, TileInfo::from(format))
            }
        };

        Ok(Self {
            id,
            client,
            urls,
            tilejson,
            tile_info,
            cache_zoom,
            upstream,
            transport,
        })
    }

    /// Fetch a single tile from the upstream, mapping its status into the cache contract:
    /// 404/204 -> empty tile, 5xx/other non-success -> error, 2xx -> bytes plus detected info/etag.
    async fn fetch(&self, xyz: TileCoord) -> Result<FetchedTile, PassthroughError> {
        let url = substitute(select_url(&self.urls, xyz), xyz);
        let response = self.client.get(&url).send().await?;
        let status = response.status();

        if status == StatusCode::NOT_FOUND || status == StatusCode::NO_CONTENT {
            return Ok(FetchedTile {
                data: TileData::new(),
                info: self.tile_info,
                etag: None,
            });
        }
        if !status.is_success() {
            return Err(PassthroughError::UnexpectedStatus {
                url,
                status: status.as_u16(),
            });
        }

        let etag = header_str(response.headers(), &ETAG).map(|raw| normalize_etag(&raw));
        let content_type = header_str(response.headers(), &CONTENT_TYPE);
        let content_encoding = header_str(response.headers(), &CONTENT_ENCODING);
        let data = response.bytes().await?.to_vec();
        let info = response_tile_info(
            self.tile_info.format,
            content_type.as_deref(),
            content_encoding.as_deref(),
            &data,
        );
        Ok(FetchedTile { data, info, etag })
    }
}

#[async_trait]
impl Source for PassthroughSource {
    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_tilejson(&self) -> &TileJSON {
        &self.tilejson
    }

    fn get_tile_info(&self) -> TileInfo {
        self.tile_info
    }

    fn clone_source(&self) -> BoxedSource {
        Box::new(self.clone())
    }

    fn benefits_from_concurrent_scraping(&self) -> bool {
        true
    }

    fn cache_zoom(&self) -> CacheZoomRange {
        self.cache_zoom
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        Ok(self.fetch(xyz).await?.data)
    }

    async fn get_tile_with_etag(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<Tile> {
        let fetched = self.fetch(xyz).await?;
        let tile = match fetched.etag {
            Some(etag) if !fetched.data.is_empty() => {
                Tile::new_with_etag(fetched.data, fetched.info, etag)
            }
            _ => Tile::new_hash_etag(fetched.data, fetched.info),
        };
        Ok(tile)
    }

    async fn try_reload(&self) -> MartinCoreResult<BoxedSource> {
        Self::new(
            self.id.clone(),
            self.upstream.clone(),
            self.transport.clone(),
            self.cache_zoom,
        )
        .await
        .map(|s| Box::new(s) as BoxedSource)
        .map_err(MartinCoreError::from)
    }
}

/// Build a pooled `reqwest::Client` with the transport headers and timeout baked in.
fn build_client(transport: &Transport) -> Result<reqwest::Client, PassthroughError> {
    let mut header_map = transport.headers.clone();
    if !header_map.contains_key(USER_AGENT) {
        header_map.insert(
            USER_AGENT,
            HeaderValue::from_static(concat!("martin/", env!("CARGO_PKG_VERSION"))),
        );
    }
    reqwest::Client::builder()
        .timeout(transport.timeout)
        .default_headers(header_map)
        .build()
        .map_err(PassthroughError::Http)
}

/// Fetch and parse an upstream `TileJSON` document.
async fn fetch_tilejson(client: &reqwest::Client, url: &str) -> Result<TileJSON, PassthroughError> {
    let response =
        client
            .get(url)
            .send()
            .await
            .map_err(|source| PassthroughError::TileJsonFetch {
                url: url.to_string(),
                source,
            })?;
    let status = response.status();
    if !status.is_success() {
        return Err(PassthroughError::TileJsonStatus {
            url: url.to_string(),
            status: status.as_u16(),
        });
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|source| PassthroughError::TileJsonFetch {
            url: url.to_string(),
            source,
        })?;
    serde_json::from_slice(&bytes).map_err(|source| PassthroughError::TileJsonParse {
        url: url.to_string(),
        source,
    })
}

/// Build the `TileJSON` served for a template source from its templates and operator-declared metadata.
fn build_template_tilejson(templates: &[String], meta: &TemplateMeta) -> TileJSON {
    let mut tj = tilejson! { tiles: templates.to_vec() };
    tj.minzoom = meta.minzoom;
    tj.maxzoom = meta.maxzoom;
    tj.bounds = meta.bounds;
    tj.attribution.clone_from(&meta.attribution);
    tj
}

/// Determine a response's [`TileInfo`] from its headers, falling back to a byte sniff and finally
/// the source-level `declared` format. The upstream `Content-Encoding` is preserved verbatim.
fn response_tile_info(
    declared: Format,
    content_type: Option<&str>,
    content_encoding: Option<&str>,
    body: &[u8],
) -> TileInfo {
    let encoding = content_encoding
        .and_then(Encoding::parse)
        .unwrap_or(Encoding::Uncompressed);
    let format = content_type
        .and_then(content_type_format)
        .or_else(|| sniff_format(body))
        .unwrap_or(declared);
    TileInfo::new(format, encoding)
}

/// Parse a `Content-Type` header value (ignoring any `; charset=…` suffix) into a [`Format`].
fn content_type_format(content_type: &str) -> Option<Format> {
    let mime = content_type.split(';').next()?.trim();
    let (supertype, subtype) = mime.split_once('/')?;
    Format::from_content_type(supertype.trim(), subtype.trim())
}

/// Sniff the logical format from the (possibly compressed) bytes; `None` only for an empty body.
fn sniff_format(body: &[u8]) -> Option<Format> {
    if body.is_empty() {
        None
    } else {
        Some(TileInfo::detect(body).format)
    }
}

/// Read a header as an owned `String`, ignoring values that are not valid UTF-8.
fn header_str(headers: &HeaderMap, name: &HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string)
}

/// Reduce an HTTP `ETag` header value to its opaque tag, dropping the weak prefix and
/// surrounding quotes (`W/"abc"` and `"abc"` both become `abc`).
///
/// Martin stores etags unquoted internally and re-adds the quotes when serving, so keeping
/// the wire quotes here would double-quote the served `ETag` and reject otherwise-valid tags.
fn normalize_etag(raw: &str) -> String {
    raw.strip_prefix("W/")
        .unwrap_or(raw)
        .trim_matches('"')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_string_headers_validates_and_collects() {
        let transport = Transport::from_string_headers(
            Duration::from_secs(1),
            [
                ("x-api-key", "secret"),
                ("accept", "application/x-protobuf"),
            ],
        )
        .unwrap();
        assert_eq!(transport.headers.get("x-api-key").unwrap(), "secret");
        assert_eq!(
            transport.headers.get("accept").unwrap(),
            "application/x-protobuf"
        );
    }

    #[test]
    fn from_string_headers_rejects_invalid_name() {
        let err = Transport::from_string_headers(Duration::from_secs(1), [("bad name", "v")])
            .unwrap_err();
        assert!(matches!(
            err,
            PassthroughError::InvalidHeaderName { name, .. } if name == "bad name"
        ));
    }

    #[test]
    fn from_string_headers_rejects_invalid_value() {
        let err = Transport::from_string_headers(Duration::from_secs(1), [("x-key", "bad\nvalue")])
            .unwrap_err();
        assert!(matches!(
            err,
            PassthroughError::InvalidHeaderValue { name, .. } if name == "x-key"
        ));
    }

    #[test]
    fn normalize_etag_strips_quotes_and_weak_prefix() {
        assert_eq!(normalize_etag("\"abc\""), "abc");
        assert_eq!(normalize_etag("W/\"abc\""), "abc");
        assert_eq!(normalize_etag("abc"), "abc");
    }
}
