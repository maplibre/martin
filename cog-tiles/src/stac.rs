//! The COGs a STAC API finds for each tile.

use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;
use reqwest::header::{CONTENT_TYPE, HeaderValue, USER_AGENT};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::{
    AsyncIndexedMosaic, CogError, CogResult, HrefResolver, IndexedMosaicOptions, MosaicFile, MosaicIndex, TileCoord,
    mercator_to_lnglat,
};

/// How long a search result is kept by default before searching again.
pub const DEFAULT_SEARCH_EXPIRY: Duration = Duration::from_hours(24);

/// How many search results are kept at once by default.
pub const DEFAULT_MAX_CACHED_SEARCHES: u64 = 10_000;

/// How many items one search reads by default.
pub const DEFAULT_MAX_ITEMS_PER_SEARCH: NonZeroUsize = NonZeroUsize::new(1000).expect("1000 != 0");

/// Items asked for per page of search results.
const PAGE_SIZE: usize = 250;

/// How many redirects within the STAC API's origin one request follows.
const MAX_REDIRECTS: usize = 10;

/// How long one request to the STAC API may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The bands served when none are configured, by `eo:bands` common name.
const DEFAULT_BANDS: [&str; 3] = ["red", "green", "blue"];

/// Which items cover a pixel first, by their `datetime`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StacOrder {
    /// The newest item is painted first; older items only fill its gaps.
    #[default]
    Newest,
    /// The oldest item is painted first; newer items only fill its gaps.
    Oldest,
}

impl StacOrder {
    /// The `sortby` direction of this order.
    fn direction(self) -> &'static str {
        match self {
            Self::Newest => "desc",
            Self::Oldest => "asc",
        }
    }
}

/// One band of an asset, by number or by `eo:bands` name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StacBand {
    /// The band's number, counted from 1.
    Number(usize),
    /// The band's `common_name` or `name` in the asset's `eo:bands`, matched without case.
    Name(String),
}

impl fmt::Display for StacBand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(number) => write!(f, "{number}"),
            Self::Name(name) => f.write_str(name),
        }
    }
}

/// Where and how a STAC API is searched for the COGs of each tile.
#[derive(Clone, Debug, PartialEq)]
pub struct StacSearchOptions {
    /// The STAC API root, e.g. `https://earth-search.aws.element84.com/v1`.
    pub url: String,
    /// The `User-Agent` sent to the STAC API, e.g. `martin/1.0.0`.
    pub user_agent: String,
    /// The collection searched.
    pub collection: String,
    /// The key of the asset read from each item.
    pub asset: String,
    /// A STAC `datetime` interval the items must fall in, e.g. `2020-01-01T00:00:00Z/..`.
    pub datetime: Option<String>,
    /// A STAC query extension object the items must match, sent as is.
    pub query: Option<Map<String, Value>>,
    /// Which items cover a pixel first.
    pub order: StacOrder,
    /// One search covers one tile at this zoom, and serves every tile below it.
    pub search_zoom: u8,
    /// The most items one search reads, kept or skipped; the rest of its pages are not fetched.
    pub max_items_per_search: NonZeroUsize,
    /// How long a search result is kept before searching again.
    pub search_expiry: Duration,
    /// The most search results kept at once.
    pub max_cached_searches: u64,
}

/// Finds the COGs of a tile by searching a STAC API, one search per tile at the search zoom.
pub struct StacIndex {
    /// HTTP client for the STAC API.
    client: reqwest::Client,
    /// The `/search` endpoint.
    search_url: String,
    /// What to search and how.
    options: StacSearchOptions,
    /// Turns each item's asset into a file.
    assets: AssetFiles,
    /// Search results by tile at the search zoom.
    searches: Cache<TileCoord, Arc<[MosaicFile]>>,
    /// The selected bands, counted from 0.
    bands: Vec<usize>,
    /// The collection's overall bbox in degrees, if it has one.
    bounds: Option<[f64; 4]>,
    /// The file of the first matching item, opened first to check that the mosaic can be served.
    probe: MosaicFile,
}

/// Turns the asset of an item into a file.
struct AssetFiles {
    /// Turns asset hrefs into stores.
    resolve: HrefResolver,
    /// The selected sample indexes with the `eo:bands` entry each was chosen by, which every
    /// asset that names its bands must match.
    band_layout: Vec<(usize, EoBand)>,
}

impl StacSearchOptions {
    /// The search request body: the filters, the order, and the bbox if any.
    fn search_body(&self, bbox: Option<[f64; 4]>) -> Map<String, Value> {
        let direction = self.order.direction();
        let mut body = Map::new();
        body.insert("collections".to_owned(), json!([self.collection]));
        if let Some(bbox) = bbox {
            body.insert("bbox".to_owned(), json!(bbox));
        }
        if let Some(datetime) = &self.datetime {
            body.insert("datetime".to_owned(), Value::from(datetime.clone()));
        }
        if let Some(query) = &self.query {
            body.insert("query".to_owned(), Value::Object(query.clone()));
        }
        // Sorting by `id` too keeps items of the same `datetime` in one order across pages:
        // Earth Search pages by the sort keys alone, and drops items whose keys tie.
        body.insert(
            "sortby".to_owned(),
            json!([
                {"field": "properties.datetime", "direction": direction},
                {"field": "id", "direction": direction},
            ]),
        );
        body.insert(
            "limit".to_owned(),
            Value::from(PAGE_SIZE.min(self.max_items_per_search.get())),
        );
        body
    }
}

impl AssetFiles {
    /// The file of `item`'s asset `key`, or `None` (with a debug log) if it has no bbox or no such
    /// asset, or its href cannot be resolved.
    fn file(&self, item: &Item, key: &str) -> Option<MosaicFile> {
        let bounds = item.bounds()?;
        let Some(asset) = item.assets.get(key) else {
            tracing::debug!(stac.item = item.id, asset = key, "STAC item has no such asset");
            return None;
        };
        if let Some(names) = &asset.eo_bands
            && let Some((band, _)) = self
                .band_layout
                .iter()
                .find(|(band, expected)| names.get(*band).is_none_or(|name| !name.same(expected)))
        {
            tracing::debug!(
                stac.item = item.id,
                asset = key,
                band = band + 1,
                "STAC asset orders its bands unlike the first item"
            );
            return None;
        }
        match (self.resolve)(&asset.href) {
            Ok((store, path)) => Some(MosaicFile {
                cache_key: without_fragment(&asset.href).into(),
                location: without_query(&asset.href).into(),
                store,
                path,
                bounds,
            }),
            Err(error) => {
                tracing::warn!(stac.item = item.id, href = asset.href, %error, "Cannot read a STAC asset");
                None
            }
        }
    }
}

impl fmt::Debug for StacIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StacIndex")
            .field("search_url", &self.search_url)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl MosaicIndex for StacIndex {
    async fn files_for(&self, coord: TileCoord) -> CogResult<Arc<[MosaicFile]>> {
        let Some(shift) = coord.z().checked_sub(self.options.search_zoom) else {
            return Ok(Arc::from([]));
        };
        let cell = TileCoord::new(self.options.search_zoom, coord.x() >> shift, coord.y() >> shift)?;
        self.searches
            .try_get_with(cell, self.search_cell(cell))
            .await
            .map_err(|source| CogError::MosaicIndex {
                location: self.search_url.clone(),
                source: Box::new(source),
            })
    }

    fn bounds(&self) -> Option<[f64; 4]> {
        self.bounds
    }
}

impl StacIndex {
    /// Opens an index over `options` serving `bands` (`None`: `red`, `green`, `blue`, or the first
    /// three), reading the collection and its first matching item now.
    ///
    /// # Errors
    /// Returns an error if the API cannot be read, the collection has no matching item, a band is
    /// not found, or the first item has no usable bbox or asset.
    pub async fn open(
        options: StacSearchOptions,
        bands: Option<&[StacBand]>,
        resolve: HrefResolver,
    ) -> CogResult<Self> {
        let client = client(&options)?;
        let search_url = format!("{}/search", options.url.trim_end_matches('/'));
        let collection = read_collection(&client, &options).await?;
        let first = first_item(&client, &search_url, &options).await?;
        let names = [
            collection.item_assets.get(&options.asset),
            first.assets.get(&options.asset),
        ]
        .into_iter()
        .flatten()
        .find_map(|asset| asset.eo_bands.clone());
        let bands = select_bands(bands, names.as_deref(), &options.asset)?;
        let assets = AssetFiles {
            resolve,
            band_layout: bands
                .iter()
                .filter_map(|&band| Some((band, names.as_deref()?.get(band)?.clone())))
                .collect(),
        };
        let probe = assets
            .file(&first, &options.asset)
            .ok_or_else(|| CogError::StacUnusableItem {
                item: first.id.clone(),
                asset: options.asset.clone(),
            })?;
        let searches = Cache::builder()
            .max_capacity(options.max_cached_searches)
            .time_to_live(options.search_expiry)
            .build();
        Ok(Self {
            client,
            search_url,
            options,
            assets,
            searches,
            bands,
            bounds: collection.bounds(),
            probe,
        })
    }

    /// The selected bands, counted from 0, to open the mosaic with.
    #[must_use]
    pub fn bands(&self) -> &[usize] {
        &self.bands
    }

    /// Searches every page of the items over `cell`, up to the item limit.
    async fn search_cell(&self, cell: TileCoord) -> CogResult<Arc<[MosaicFile]>> {
        let [min_x, min_y, max_x, max_y] = cell.mercator_bounds();
        let [west, south] = mercator_to_lnglat(min_x, min_y);
        let [east, north] = mercator_to_lnglat(max_x, max_y);
        let mut request = Request::Post {
            url: self.search_url.clone(),
            body: self.options.search_body(Some([west, south, east, north])),
        };
        let max_items = self.options.max_items_per_search.get();
        let mut files = Vec::new();
        let mut skipped = 0_usize;
        let mut read = 0_usize;
        let mut seen = 0_usize;
        let truncated = 'pages: loop {
            let page: ItemCollection = match &request {
                Request::Post { url, body } => post(&self.client, url, body).await?,
                Request::Get { url } => read_json(url, self.client.get(url).send().await).await?,
            };
            let page_len = page.features.len();
            seen += page_len;
            let matched = page.number_matched.or(page.context.and_then(|context| context.matched));
            for item in page.features {
                if read == max_items {
                    break 'pages true;
                }
                read += 1;
                match self.assets.file(&item, &self.options.asset) {
                    Some(file) => files.push(file),
                    None => skipped += 1,
                }
            }
            // Earth Search links a next page even after the last item, so stop once all are seen,
            // or at an empty page if the API does not say how many match.
            let next = page
                .links
                .into_iter()
                .find(|link| link.rel == "next")
                .filter(|_| page_len > 0 && matched.is_none_or(|matched| seen < matched));
            match next {
                Some(_) if read == max_items => break true,
                Some(next) => {
                    request = request.next(next);
                    if !same_origin(request.url(), &self.options.url) {
                        return Err(CogError::StacForeignLink {
                            api: self.options.url.clone(),
                            link: request.url().to_owned(),
                        });
                    }
                }
                None => break false,
            }
        };
        if skipped > 0 {
            tracing::warn!(
                stac.search = %self.search_url,
                tile = %cell,
                skipped,
                "Skipping STAC items without a usable bbox or asset"
            );
        }
        if truncated {
            tracing::warn!(
                stac.search = %self.search_url,
                tile = %cell,
                max_items_per_search = max_items,
                "A STAC search found more items than it reads; the rest are not painted"
            );
        }
        Ok(files.into())
    }
}

impl AsyncIndexedMosaic {
    /// Opens a mosaic of the COGs `index` finds, checking now that its first item's file can be served.
    ///
    /// # Errors
    /// Returns an error if the min zoom is below the search zoom, or see [`open`](Self::open).
    pub async fn open_stac(index: StacIndex, options: IndexedMosaicOptions) -> CogResult<Self> {
        let search_zoom = index.options.search_zoom;
        if options.min_zoom < search_zoom {
            return Err(CogError::StacMinZoom {
                min_zoom: options.min_zoom,
                search_zoom,
            });
        }
        let probe = index.probe.clone();
        Self::open(Arc::new(index), &probe, options).await
    }
}

/// An HTTP client for the STAC API at `options.url`, sending `options.user_agent`.
fn client(options: &StacSearchOptions) -> CogResult<reqwest::Client> {
    let api = options.url.clone();
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() < MAX_REDIRECTS && same_origin(attempt.url().as_str(), &api) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .default_headers(
            [(
                USER_AGENT,
                HeaderValue::from_str(&options.user_agent).map_err(|header| CogError::StacUserAgent {
                    user_agent: options.user_agent.clone(),
                    header,
                })?,
            )]
            .into_iter()
            .collect(),
        )
        .build()
        .map_err(|source| CogError::StacRequest {
            url: options.url.clone(),
            source,
        })
}

/// The metadata of the collection `options` searches.
async fn read_collection(client: &reqwest::Client, options: &StacSearchOptions) -> CogResult<Collection> {
    let url = format!(
        "{}/collections/{}",
        options.url.trim_end_matches('/'),
        options.collection
    );
    read_json(&url, client.get(&url).send().await).await
}

/// The first item at `search_url` that matches the filters of `options`.
async fn first_item(client: &reqwest::Client, search_url: &str, options: &StacSearchOptions) -> CogResult<Item> {
    let mut body = options.search_body(None);
    body.insert("limit".to_owned(), Value::from(1));
    let page: ItemCollection = post(client, search_url, &body).await?;
    page.features.into_iter().next().ok_or_else(|| CogError::StacEmpty {
        url: search_url.to_owned(),
        collection: options.collection.clone(),
    })
}

/// POSTs `body` as JSON to `url`.
async fn post<T: DeserializeOwned>(client: &reqwest::Client, url: &str, body: &Map<String, Value>) -> CogResult<T> {
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .body(Value::Object(body.clone()).to_string());
    read_json(url, request.send().await).await
}

/// The 0-based sample indexes of `bands`, by the asset's `eo:bands` `names` if any.
fn select_bands(bands: Option<&[StacBand]>, names: Option<&[EoBand]>, asset: &str) -> CogResult<Vec<usize>> {
    let defaults: Vec<StacBand>;
    let bands = match (bands, names) {
        (Some(bands), _) => bands,
        (None, Some(_)) => {
            defaults = DEFAULT_BANDS
                .iter()
                .map(|name| StacBand::Name((*name).to_owned()))
                .collect();
            &defaults
        }
        (None, None) => return Ok(vec![0, 1, 2]),
    };
    bands
        .iter()
        .map(|band| match band {
            StacBand::Number(number) if *number >= 1 => Ok(number - 1),
            StacBand::Number(_) => Err(CogError::StacBand {
                band: band.to_string(),
                asset: asset.to_owned(),
                available: "bands are counted from 1".to_owned(),
            }),
            StacBand::Name(name) => names
                .unwrap_or_default()
                .iter()
                .position(|eo| eo.is(name))
                .ok_or_else(|| CogError::StacBand {
                    band: name.clone(),
                    asset: asset.to_owned(),
                    available: available_names(names.unwrap_or_default()),
                }),
        })
        .collect()
}

/// The names of `bands`, for an error.
fn available_names(bands: &[EoBand]) -> String {
    if bands.is_empty() {
        return "the asset does not name its bands, select them by number".to_owned();
    }
    let names: Vec<_> = bands
        .iter()
        .map(|band| band.common_name.as_deref().or(band.name.as_deref()).unwrap_or("?"))
        .collect();
    format!("it has {}", names.join(", "))
}

/// Reads a JSON response, or the error the request or response stands for.
async fn read_json<T: DeserializeOwned>(
    url: &str,
    response: Result<reqwest::Response, reqwest::Error>,
) -> CogResult<T> {
    let request_error = |source| CogError::StacRequest {
        url: url.to_owned(),
        source,
    };
    let response = response.map_err(request_error)?;
    let status = response.status();
    let body = response.bytes().await.map_err(request_error)?;
    if !status.is_success() {
        return Err(CogError::StacStatus {
            url: url.to_owned(),
            status: status.as_u16(),
            body: String::from_utf8_lossy(&body).chars().take(500).collect(),
        });
    }
    serde_json::from_slice(&body).map_err(|source| CogError::StacResponse {
        url: url.to_owned(),
        source,
    })
}

/// The next request of a paged search.
enum Request {
    /// A POST of a search body.
    Post {
        /// Where to POST.
        url: String,
        /// The search body.
        body: Map<String, Value>,
    },
    /// A GET of a URL carrying the search.
    Get {
        /// What to GET.
        url: String,
    },
}

impl Request {
    /// Where this request goes.
    fn url(&self) -> &str {
        match self {
            Self::Post { url, .. } | Self::Get { url } => url,
        }
    }

    /// The request a `next` link stands for, following this one. A relative `href` is resolved
    /// against this request's URL.
    fn next(self, link: Link) -> Self {
        let href = reqwest::Url::parse(self.url())
            .and_then(|base| base.join(&link.href))
            .map_or(link.href, String::from);
        let post = link
            .method
            .as_deref()
            .is_some_and(|method| method.eq_ignore_ascii_case("POST"));
        if !post {
            return Self::Get { url: href };
        }
        let previous = match self {
            Self::Post { body, .. } => body,
            Self::Get { .. } => Map::new(),
        };
        let body = match (link.body, link.merge) {
            (Some(next), true) => {
                let mut body = previous;
                body.extend(next);
                body
            }
            (Some(next), false) => next,
            (None, _) => previous,
        };
        Self::Post { url: href, body }
    }
}

/// A STAC collection, as far as a source reads it.
#[derive(Debug, Deserialize)]
struct Collection {
    /// The collection's extent.
    #[serde(default)]
    extent: Option<Extent>,
    /// What the assets of every item hold, by asset key.
    #[serde(default)]
    item_assets: HashMap<String, Asset>,
}

impl Collection {
    /// The collection's overall bbox in degrees, if it has one.
    fn bounds(&self) -> Option<[f64; 4]> {
        let bbox = self.extent.as_ref()?.spatial.bbox.first()?;
        bbox_bounds(bbox)
    }
}

/// The extent of a collection.
#[derive(Debug, Deserialize)]
struct Extent {
    /// Its spatial extent.
    spatial: SpatialExtent,
}

/// The spatial extent of a collection.
#[derive(Debug, Deserialize)]
struct SpatialExtent {
    /// Bboxes, the first covering all items.
    bbox: Vec<Vec<f64>>,
}

/// A page of search results.
#[derive(Debug, Deserialize)]
struct ItemCollection {
    /// The items.
    #[serde(default)]
    features: Vec<Item>,
    /// Links, among them the next page.
    #[serde(default)]
    links: Vec<Link>,
    /// How many items match the search in all, if the API says.
    #[serde(default, rename = "numberMatched")]
    number_matched: Option<usize>,
    /// The older form of `numberMatched`, from the STAC context extension.
    #[serde(default)]
    context: Option<SearchContext>,
}

/// The STAC context extension of a page of search results.
#[derive(Debug, Deserialize)]
struct SearchContext {
    /// How many items match the search in all.
    #[serde(default)]
    matched: Option<usize>,
}

/// One STAC item.
#[derive(Debug, Deserialize)]
struct Item {
    /// The item ID.
    id: String,
    /// The item's bbox, 2D or 3D.
    #[serde(default)]
    bbox: Option<Vec<f64>>,
    /// The item's assets, by key.
    #[serde(default)]
    assets: HashMap<String, Asset>,
}

impl Item {
    /// The item's bbox in degrees, or `None` (with a debug log) if it has none.
    fn bounds(&self) -> Option<[f64; 4]> {
        let bounds = self.bbox.as_deref().and_then(bbox_bounds);
        if bounds.is_none() {
            tracing::debug!(stac.item = self.id, "STAC item has no usable bbox");
        }
        bounds
    }
}

/// One asset of an item, or of every item in `item_assets`.
#[derive(Clone, Debug, Deserialize)]
struct Asset {
    /// Where the asset is. Absent in `item_assets`.
    #[serde(default)]
    href: String,
    /// The bands of the asset.
    #[serde(default, rename = "eo:bands")]
    eo_bands: Option<Vec<EoBand>>,
}

/// One band in `eo:bands`.
#[derive(Clone, Debug, Deserialize)]
struct EoBand {
    /// The band's name.
    #[serde(default)]
    name: Option<String>,
    /// The band's common name, e.g. `red`.
    #[serde(default)]
    common_name: Option<String>,
}

impl EoBand {
    /// Whether this band goes by `name`, ignoring case.
    fn is(&self, name: &str) -> bool {
        [&self.common_name, &self.name]
            .into_iter()
            .flatten()
            .any(|own| own.eq_ignore_ascii_case(name))
    }

    /// Whether this band goes by any name of `other`.
    fn same(&self, other: &Self) -> bool {
        [&other.common_name, &other.name]
            .into_iter()
            .flatten()
            .any(|name| self.is(name))
    }
}

/// A link in a search result.
#[derive(Debug, Deserialize)]
struct Link {
    /// The link relation, `next` for the next page.
    rel: String,
    /// Where the link points.
    href: String,
    /// The HTTP method, GET when absent.
    #[serde(default)]
    method: Option<String>,
    /// The body to POST.
    #[serde(default)]
    body: Option<Map<String, Value>>,
    /// Whether `body` is merged into the previous request body.
    #[serde(default)]
    merge: bool,
}

/// `href` without its query and fragment, which may hold credentials, e.g. of a presigned URL.
fn without_query(href: &str) -> &str {
    href.split(['?', '#']).next().unwrap_or(href)
}

/// `href` up to its fragment, which never selects a different file.
fn without_fragment(href: &str) -> &str {
    href.split('#').next().unwrap_or(href)
}

/// `[west, south, east, north]` of a 2D or 3D STAC bbox, all longitudes if it crosses the date line.
fn bbox_bounds(bbox: &[f64]) -> Option<[f64; 4]> {
    match *bbox {
        [west, south, east, north] | [west, south, _, east, north, _] if west > east => {
            Some([-180.0, south, 180.0, north])
        }
        [west, south, east, north] | [west, south, _, east, north, _] => Some([west, south, east, north]),
        _ => None,
    }
}

/// Whether `url` has the scheme, host and port of `api`.
fn same_origin(url: &str, api: &str) -> bool {
    match (reqwest::Url::parse(url), reqwest::Url::parse(api)) {
        (Ok(url), Ok(api)) => url.origin() == api.origin(),
        _ => false,
    }
}
