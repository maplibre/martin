//! Tile sources over the COGs a STAC API finds for each tile, reprojected onto Web Mercator.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use cog_tiles::{AsyncIndexedMosaic, IndexedMosaicOptions, MosaicFile, MosaicIndex, mercator_to_lnglat};
use moka::future::Cache;
use object_store::ObjectStore;
use object_store::path::Path;
use reqwest::header::{CONTENT_TYPE, HeaderValue, USER_AGENT};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use tilejson::{Bounds, Center, tilejson};

use crate::CacheZoomRange;
use crate::tiles::cog::CogError;
use crate::tiles::cog::source::{CogSource, CogTiles};

/// Items asked for per page of search results.
const PAGE_SIZE: usize = 250;

/// How long one request to the STAC API may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The bands served when none are configured, by `eo:bands` common name.
const DEFAULT_BANDS: [&str; 3] = ["red", "green", "blue"];

/// Turns an asset `href` into the store and path it is read from.
pub type HrefResolver =
    Arc<dyn Fn(&str) -> Result<(Arc<dyn ObjectStore>, Path), Box<dyn std::error::Error + Send + Sync>> + Send + Sync>;

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
    pub max_items_per_search: usize,
    /// How long a search result is kept before searching again.
    pub search_expiry: Duration,
    /// The most search results kept at once.
    pub max_cached_searches: u64,
}

/// A source over the COGs a STAC API finds for each tile: what to search, and how to paint.
#[derive(Clone, Debug, PartialEq)]
pub struct CogStacOptions {
    /// What to search and how.
    pub search: StacSearchOptions,
    /// The lowest zoom served, at or above [`StacSearchOptions::search_zoom`].
    pub min_zoom: u8,
    /// The highest zoom served. `None` uses the zoom at which the first item found is fully detailed.
    pub max_zoom: Option<u8>,
    /// The bands served: one for gray, three for RGB. `None` serves the bands named `red`, `green`
    /// and `blue`, or the first three if the asset does not name its bands.
    pub bands: Option<Vec<StacBand>>,
    /// The sample value of pixels without data, overriding each file's `GDAL_NODATA`.
    pub nodata: Option<u8>,
    /// The most opened files kept at once.
    pub max_cached_files: u64,
    /// How long an opened file is kept, so replaced files are picked up eventually.
    pub file_expiry: Duration,
    /// How long an opened file is kept without being used. `None` keeps it until `file_expiry`.
    pub file_idle_timeout: Option<Duration>,
}

/// Finds the COGs of a tile by searching a STAC API, one search per tile at the search zoom.
pub struct StacIndex {
    /// HTTP client for the STAC API.
    client: reqwest::Client,
    /// The `/search` endpoint.
    search_url: String,
    /// What to search and how.
    options: StacSearchOptions,
    /// Turns asset hrefs into stores.
    resolve: HrefResolver,
    /// Search results by tile at the search zoom.
    searches: Cache<cog_tiles::TileCoord, Arc<[MosaicFile]>>,
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
    async fn files_for(&self, coord: cog_tiles::TileCoord) -> cog_tiles::CogResult<Arc<[MosaicFile]>> {
        let Some(shift) = coord.z().checked_sub(self.options.search_zoom) else {
            return Ok(Arc::from([]));
        };
        let cell = cog_tiles::TileCoord::new(self.options.search_zoom, coord.x() >> shift, coord.y() >> shift)?;
        self.searches
            .try_get_with(cell, self.search_cell(cell))
            .await
            .map_err(|source| cog_tiles::CogError::MosaicIndex {
                location: self.search_url.clone(),
                source: Box::new(source),
            })
    }
}

impl StacIndex {
    /// An index over `options`, resolving asset hrefs with `resolve`.
    ///
    /// # Errors
    /// Returns an error if the HTTP client cannot be built.
    pub fn new(options: StacSearchOptions, resolve: HrefResolver) -> Result<Self, CogError> {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .default_headers(
                [(
                    USER_AGENT,
                    HeaderValue::from_static(concat!("martin/", env!("CARGO_PKG_VERSION"))),
                )]
                .into_iter()
                .collect(),
            )
            .build()
            .map_err(|source| CogError::StacRequest {
                url: options.url.clone(),
                source,
            })?;
        let searches = Cache::builder()
            .max_capacity(options.max_cached_searches)
            .time_to_live(options.search_expiry)
            .build();
        Ok(Self {
            client,
            search_url: format!("{}/search", options.url.trim_end_matches('/')),
            options,
            resolve,
            searches,
        })
    }

    /// The collection's metadata.
    async fn collection(&self) -> Result<Collection, CogError> {
        let url = format!(
            "{}/collections/{}",
            self.options.url.trim_end_matches('/'),
            self.options.collection
        );
        let response = self.client.get(&url).send().await;
        read_json(&url, response).await
    }

    /// The first item of the collection that matches the filters.
    async fn first_item(&self) -> Result<Item, CogError> {
        let mut body = self.search_body(None);
        body.insert("limit".to_owned(), Value::from(1));
        let page: ItemCollection = self.post(&self.search_url, &body).await?;
        page.features.into_iter().next().ok_or_else(|| CogError::StacEmpty {
            url: self.search_url.clone(),
            collection: self.options.collection.clone(),
        })
    }

    /// Searches every page of the items over `cell`, up to the item limit.
    async fn search_cell(&self, cell: cog_tiles::TileCoord) -> Result<Arc<[MosaicFile]>, CogError> {
        let [min_x, min_y, max_x, max_y] = cell.mercator_bounds();
        let [west, south] = mercator_to_lnglat(min_x, min_y);
        let [east, north] = mercator_to_lnglat(max_x, max_y);
        let mut request = Request::Post {
            url: self.search_url.clone(),
            body: self.search_body(Some([west, south, east, north])),
        };
        let max_items = self.options.max_items_per_search;
        let mut files = Vec::new();
        let mut skipped = 0_usize;
        let mut read = 0_usize;
        let mut seen = 0_usize;
        let truncated = 'pages: loop {
            let page: ItemCollection = match &request {
                Request::Post { url, body } => self.post(url, body).await?,
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
                match self.file(&item) {
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
                Some(next) => request = request.next(next),
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

    /// The search request body: the filters, the order, and the bbox if any.
    fn search_body(&self, bbox: Option<[f64; 4]>) -> Map<String, Value> {
        let direction = self.options.order.direction();
        let mut body = Map::new();
        body.insert("collections".to_owned(), json!([self.options.collection]));
        if let Some(bbox) = bbox {
            body.insert("bbox".to_owned(), json!(bbox));
        }
        if let Some(datetime) = &self.options.datetime {
            body.insert("datetime".to_owned(), Value::from(datetime.clone()));
        }
        if let Some(query) = &self.options.query {
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
            Value::from(PAGE_SIZE.min(self.options.max_items_per_search)),
        );
        body
    }

    /// POSTs `body` as JSON to `url`.
    async fn post<T: DeserializeOwned>(&self, url: &str, body: &Map<String, Value>) -> Result<T, CogError> {
        let request = self
            .client
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .body(Value::Object(body.clone()).to_string());
        read_json(url, request.send().await).await
    }

    /// The file of `item`'s asset, or `None` (with a debug log) if it has no bbox or no such asset,
    /// or its href cannot be resolved.
    fn file(&self, item: &Item) -> Option<MosaicFile> {
        let bounds = item.bounds()?;
        let Some(asset) = item.assets.get(&self.options.asset) else {
            tracing::debug!(
                stac.item = item.id,
                asset = self.options.asset,
                "STAC item has no such asset"
            );
            return None;
        };
        match (self.resolve)(&asset.href) {
            Ok((store, path)) => Some(MosaicFile {
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

impl CogSource {
    /// Creates a source over the COGs a STAC API finds for each tile, resolving their hrefs with
    /// `resolve`.
    ///
    /// The collection is read now for its extent and band names, and its first matching item is
    /// opened to check that it can be served.
    ///
    /// # Errors
    /// Returns an error if the min zoom is below the search zoom, the API cannot be read, the
    /// collection has no matching item, a band is not found, or the first item cannot be served.
    pub async fn new_stac(
        id: String,
        options: CogStacOptions,
        resolve: HrefResolver,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, CogError> {
        if options.min_zoom < options.search.search_zoom {
            return Err(CogError::StacMinZoom {
                min_zoom: options.min_zoom,
                search_zoom: options.search.search_zoom,
            });
        }
        let location = format!("{}/collections/{}", options.search.url, options.search.collection);
        let asset_key = options.search.asset.clone();
        let index = StacIndex::new(options.search, resolve)?;
        let collection = index.collection().await?;
        let first = index.first_item().await?;
        let names = [collection.item_assets.get(&asset_key), first.assets.get(&asset_key)]
            .into_iter()
            .flatten()
            .find_map(|asset| asset.eo_bands.clone());
        let bands = select_bands(options.bands.as_deref(), names.as_deref(), &asset_key)?;
        let probe = index.file(&first).ok_or_else(|| CogError::StacUnusableItem {
            item: first.id.clone(),
            asset: asset_key,
        })?;

        let mosaic_options = IndexedMosaicOptions::new(options.min_zoom, bands)
            .with_max_zoom(options.max_zoom)
            .with_nodata(options.nodata)
            .with_max_cached_files(options.max_cached_files)
            .with_file_expiry(options.file_expiry)
            .with_file_idle_timeout(options.file_idle_timeout);
        let mosaic = AsyncIndexedMosaic::open(Arc::new(index), &probe, mosaic_options).await?;
        let info = *mosaic.info();
        let [west, south, east, north] = collection.bounds().unwrap_or([-180.0, -85.051_129, 180.0, 85.051_129]);
        let tilejson = tilejson! {
            tiles: vec![],
            bounds: Bounds::new(west, south, east, north),
            center: Center {
                longitude: f64::midpoint(west, east),
                latitude: f64::midpoint(south, north),
                zoom: info.min_zoom,
            },
            minzoom: info.min_zoom,
            maxzoom: info.max_zoom,
        };
        Self::with_layout(
            id,
            CogTiles::Indexed(Arc::new(mosaic)),
            tilejson,
            (info.tile_size, info.format),
            location,
            cache_zoom,
        )
    }
}

/// The 0-based sample indexes of `bands`, by the asset's `eo:bands` `names` if any.
fn select_bands(bands: Option<&[StacBand]>, names: Option<&[EoBand]>, asset: &str) -> Result<Vec<usize>, CogError> {
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
) -> Result<T, CogError> {
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
    /// The request a `next` link stands for, following this one. A relative `href` is resolved
    /// against this request's URL.
    fn next(self, link: Link) -> Self {
        let base = match &self {
            Self::Post { url, .. } | Self::Get { url } => url,
        };
        let href = reqwest::Url::parse(base)
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

/// `[west, south, east, north]` of a 2D or 3D STAC bbox.
fn bbox_bounds(bbox: &[f64]) -> Option<[f64; 4]> {
    match *bbox {
        [west, south, east, north] | [west, south, _, east, north, _] => Some([west, south, east, north]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use martin_tile_utils::TileCoord;
    use object_store::memory::InMemory;
    use object_store::path::Path;
    use object_store::{ObjectStore, ObjectStoreExt as _, PutPayload};
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::CacheZoomRange;
    use crate::tiles::Source as _;
    use crate::tiles::cog::{CogSource, CogStacOptions, StacOrder, StacSearchOptions};

    fn item(id: &str, href: &str) -> serde_json::Value {
        json!({
            "type": "Feature",
            "id": id,
            "bbox": [-122.454_75, 37.790_5, -122.437_3, 37.802_2],
            "properties": {"datetime": "2022-05-18T16:00:00Z"},
            "assets": {"image": {"href": href}},
        })
    }

    async fn stac_source(server: &MockServer) -> CogSource {
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let utm = std::fs::read("../tests/fixtures/cog/warped/utm.tif").unwrap();
        for key in ["a.tif", "b.tif"] {
            store
                .put(&Path::from(key), PutPayload::from(utm.clone()))
                .await
                .unwrap();
        }
        CogSource::new_stac(
            "stac".to_owned(),
            CogStacOptions {
                search: StacSearchOptions {
                    url: server.uri(),
                    collection: "ortho".to_owned(),
                    asset: "image".to_owned(),
                    datetime: None,
                    query: None,
                    order: StacOrder::Newest,
                    search_zoom: 13,
                    max_items_per_search: 1000,
                    search_expiry: Duration::from_mins(1),
                    max_cached_searches: 10,
                },
                min_zoom: 13,
                max_zoom: Some(17),
                bands: None,
                nodata: None,
                max_cached_files: 10,
                file_expiry: Duration::from_mins(1),
                file_idle_timeout: None,
            },
            Arc::new(move |href: &str| {
                let key = href.strip_prefix("memory://").ok_or("not a memory:// href")?;
                Ok((Arc::clone(&store), Path::from(key)))
            }),
            CacheZoomRange::default(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_search_follows_next_links_and_merges_their_bodies() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/collections/ortho"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "extent": {"spatial": {"bbox": [[-122.5, 37.7, -122.4, 37.9]]}},
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/search"))
            .and(body_partial_json(json!({"limit": 1})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "features": [item("a", "memory://a.tif")],
            })))
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/search"))
            .and(body_partial_json(json!({"collections": ["ortho"], "next": "page-2"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "features": [item("b", "memory://b.tif")],
                "numberMatched": 2,
                "links": [{"rel": "next", "href": format!("{}/search", server.uri()), "method": "POST", "body": {"next": "page-3"}, "merge": true}],
            })))
            .with_priority(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "features": [item("a", "memory://a.tif")],
                "numberMatched": 2,
                "links": [{"rel": "next", "href": format!("{}/search", server.uri()), "method": "POST", "body": {"next": "page-2"}, "merge": true}],
            })))
            .with_priority(3)
            .mount(&server)
            .await;
        let source = stac_source(&server).await;

        let tile = source
            .get_tile(TileCoord::new_unchecked(16, 10_477, 25_326), None)
            .await
            .unwrap();

        assert!(!tile.is_empty());
        let searches: Vec<_> = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.method.as_str() == "POST")
            .map(|request| {
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                (body["limit"].clone(), body["next"].clone(), body["bbox"].is_array())
            })
            .collect();
        assert_eq!(
            searches,
            [
                (json!(1), serde_json::Value::Null, false),
                (json!(250), serde_json::Value::Null, true),
                (json!(250), json!("page-2"), true),
            ]
        );
    }

    #[tokio::test]
    async fn a_relative_next_link_is_resolved_against_the_page_it_came_from() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/collections/ortho"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "extent": {"spatial": {"bbox": [[-122.5, 37.7, -122.4, 37.9]]}},
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/search"))
            .and(body_partial_json(json!({"limit": 1})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "features": [item("a", "memory://a.tif")],
            })))
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "features": [item("a", "memory://a.tif")],
                "numberMatched": 2,
                "links": [{"rel": "next", "href": "/search?token=page-2"}],
            })))
            .with_priority(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .and(query_param("token", "page-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "features": [item("b", "memory://b.tif")],
                "numberMatched": 2,
            })))
            .expect(1)
            .mount(&server)
            .await;
        let source = stac_source(&server).await;

        let tile = source
            .get_tile(TileCoord::new_unchecked(16, 10_477, 25_326), None)
            .await
            .unwrap();

        assert!(!tile.is_empty());
    }

    #[tokio::test]
    async fn a_search_stops_after_reading_max_items_per_search_even_when_it_keeps_none() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/collections/ortho"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "extent": {"spatial": {"bbox": [[-122.5, 37.7, -122.4, 37.9]]}},
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/search"))
            .and(body_partial_json(json!({"limit": 1})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "features": [item("a", "memory://a.tif")],
            })))
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(path("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "features": [{
                    "type": "Feature",
                    "id": "no-image",
                    "bbox": [-122.454_75, 37.790_5, -122.437_3, 37.802_2],
                    "properties": {"datetime": "2022-05-18T16:00:00Z"},
                    "assets": {},
                }],
                "links": [{"rel": "next", "href": "/search?token=same"}],
            })))
            .with_priority(2)
            .mount(&server)
            .await;
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        store
            .put(
                &Path::from("a.tif"),
                PutPayload::from(std::fs::read("../tests/fixtures/cog/warped/utm.tif").unwrap()),
            )
            .await
            .unwrap();
        let source = CogSource::new_stac(
            "stac".to_owned(),
            CogStacOptions {
                search: StacSearchOptions {
                    url: server.uri(),
                    collection: "ortho".to_owned(),
                    asset: "image".to_owned(),
                    datetime: None,
                    query: None,
                    order: StacOrder::Newest,
                    search_zoom: 13,
                    max_items_per_search: 3,
                    search_expiry: Duration::from_mins(1),
                    max_cached_searches: 10,
                },
                min_zoom: 13,
                max_zoom: Some(17),
                bands: None,
                nodata: None,
                max_cached_files: 10,
                file_expiry: Duration::from_mins(1),
                file_idle_timeout: None,
            },
            Arc::new(move |href: &str| {
                let key = href.strip_prefix("memory://").ok_or("not a memory:// href")?;
                Ok((Arc::clone(&store), Path::from(key)))
            }),
            CacheZoomRange::default(),
        )
        .await
        .unwrap();

        let tile = source
            .get_tile(TileCoord::new_unchecked(16, 10_477, 25_326), None)
            .await
            .unwrap();

        assert!(tile.is_empty());
        let pages = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.url.path() == "/search")
            .count();
        assert_eq!(pages, 1 + 3);
    }

    #[tokio::test]
    async fn an_api_error_is_reported_with_its_status_and_body() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503).set_body_string("try later"))
            .mount(&server)
            .await;

        let error = CogSource::new_stac(
            "stac".to_owned(),
            CogStacOptions {
                search: StacSearchOptions {
                    url: server.uri(),
                    collection: "ortho".to_owned(),
                    asset: "image".to_owned(),
                    datetime: None,
                    query: None,
                    order: StacOrder::Newest,
                    search_zoom: 13,
                    max_items_per_search: 1000,
                    search_expiry: Duration::from_mins(1),
                    max_cached_searches: 10,
                },
                min_zoom: 13,
                max_zoom: None,
                bands: None,
                nodata: None,
                max_cached_files: 10,
                file_expiry: Duration::from_mins(1),
                file_idle_timeout: None,
            },
            Arc::new(|_href: &str| Err("unused".into())),
            CacheZoomRange::default(),
        )
        .await
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "The STAC API at {}/collections/ortho answered 503: try later",
                server.uri()
            )
        );
    }
}
