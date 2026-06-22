//! The [`PassthroughSource`] [`Source`] implementation and its HTTP fetch logic.

use std::collections::HashMap;
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
use crate::tiles::passthrough::url::{UrlSpec, derive_format, select_url, substitute};
use crate::tiles::{BoxedSource, MartinCoreError, MartinCoreResult, Source, Tile, UrlQuery};

/// Default per-request timeout when the configuration does not specify one.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Resolved configuration for a single passthrough source.
///
/// The `martin` config layer is responsible for env-var substitution in `urls`/`headers` before
/// constructing this; `martin-core` consumes the already-resolved values.
#[derive(Clone, Debug)]
pub struct PassthroughConfig {
    /// Upstream URL(s): a single `TileJSON` document URL, a single `{z}/{x}/{y}` template, or a
    /// list of templates.
    pub urls: Vec<String>,
    /// Headers sent with every request (both `TileJSON` discovery and per-tile fetches).
    pub headers: HashMap<String, String>,
    /// Per-request timeout.
    pub timeout: Duration,
    /// Explicit format override; takes precedence over URL extension and `TileJSON`.
    pub format: Option<Format>,
    /// User-declared minimum zoom (template case only; no value is fabricated).
    pub minzoom: Option<u8>,
    /// User-declared maximum zoom (template case only; no value is fabricated).
    pub maxzoom: Option<u8>,
    /// User-declared bounds (template case only).
    pub bounds: Option<Bounds>,
    /// User-declared attribution (template case only).
    pub attribution: Option<String>,
    /// Zoom range controlling which zoom levels are cached.
    pub cache_zoom: CacheZoomRange,
}

impl Default for PassthroughConfig {
    fn default() -> Self {
        Self {
            urls: Vec::new(),
            headers: HashMap::new(),
            timeout: DEFAULT_TIMEOUT,
            format: None,
            minzoom: None,
            maxzoom: None,
            bounds: None,
            attribution: None,
            cache_zoom: CacheZoomRange::default(),
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
    config: PassthroughConfig,
}

/// A tile fetched from the upstream, before it is wrapped into a [`Tile`].
struct FetchedTile {
    data: TileData,
    info: TileInfo,
    /// The upstream `ETag` header verbatim, if any (so large tiles are not re-hashed).
    etag: Option<String>,
}

impl PassthroughSource {
    /// Build a passthrough source, fetching the upstream `TileJSON` once if a document URL was given.
    pub async fn new(id: String, config: PassthroughConfig) -> Result<Self, PassthroughError> {
        let spec = UrlSpec::detect(&id, &config.urls)?;
        let client = build_client(&config.headers, config.timeout)?;

        let (urls, tilejson, tile_info) = match spec {
            UrlSpec::Templates(templates) => {
                let first = templates
                    .first()
                    .ok_or_else(|| PassthroughError::EmptyUrlList(id.clone()))?;
                let format = derive_format(&id, config.format, first, None)?;
                let tilejson = build_template_tilejson(&templates, &config);
                (templates, tilejson, TileInfo::from(format))
            }
            UrlSpec::TileJson(doc_url) => {
                let upstream = fetch_tilejson(&client, &doc_url).await?;
                let templates = upstream.tiles.clone();
                let first = templates
                    .first()
                    .ok_or_else(|| PassthroughError::NoTilesInTileJson(doc_url.clone()))?;
                let tj_format = upstream.other.get("format").and_then(|v| v.as_str());
                let format = derive_format(&id, config.format, first, tj_format)?;
                (templates, upstream, TileInfo::from(format))
            }
        };

        Ok(Self {
            id,
            client,
            urls,
            tilejson,
            tile_info,
            cache_zoom: config.cache_zoom,
            config,
        })
    }

    /// Fetch a single tile from the upstream, mapping its status into the cache contract:
    /// 404/204 → empty tile, 5xx/other non-success → error, 2xx → bytes plus detected info/etag.
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

        let etag = header_str(response.headers(), &ETAG);
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
        Self::new(self.id.clone(), self.config.clone())
            .await
            .map(|s| Box::new(s) as BoxedSource)
            .map_err(MartinCoreError::from)
    }
}

/// Build a pooled `reqwest::Client` with the configured headers and timeout baked in.
fn build_client(
    headers: &HashMap<String, String>,
    timeout: Duration,
) -> Result<reqwest::Client, PassthroughError> {
    let mut header_map = HeaderMap::with_capacity(headers.len() + 1);
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| PassthroughError::InvalidHeader(e.to_string()))?;
        let value = HeaderValue::from_str(value)
            .map_err(|e| PassthroughError::InvalidHeader(e.to_string()))?;
        header_map.insert(name, value);
    }
    if !header_map.contains_key(USER_AGENT) {
        header_map.insert(
            USER_AGENT,
            HeaderValue::from_static(concat!("martin/", env!("CARGO_PKG_VERSION"))),
        );
    }
    reqwest::Client::builder()
        .timeout(timeout)
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

/// Build the `TileJSON` served for a template source: only `tilejson` + `tiles[]` plus any
/// user-declared metadata. No defaults are fabricated and `vector_layers` is left unset.
fn build_template_tilejson(templates: &[String], config: &PassthroughConfig) -> TileJSON {
    let mut tj = tilejson! { tiles: templates.to_vec() };
    tj.minzoom = config.minzoom;
    tj.maxzoom = config.maxzoom;
    tj.bounds = config.bounds;
    tj.attribution.clone_from(&config.attribution);
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
