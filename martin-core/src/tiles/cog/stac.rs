//! Tile sources over the COGs a STAC API finds for each tile, reprojected onto Web Mercator.

use std::sync::Arc;
use std::time::Duration;

use cog_tiles::{
    AsyncIndexedMosaic, HrefResolver, IndexedMosaicOptions, MAX_LATITUDE, StacBand, StacIndex, StacSearchOptions,
};
use tilejson::{Bounds, Center, tilejson};

use crate::CacheZoomRange;
use crate::tiles::cog::CogError;
use crate::tiles::cog::source::{CogSource, CogTiles};

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
            return Err(cog_tiles::CogError::StacMinZoom {
                min_zoom: options.min_zoom,
                search_zoom: options.search.search_zoom,
            }
            .into());
        }
        let location = format!("{}/collections/{}", options.search.url, options.search.collection);
        let (index, probe) = StacIndex::open(options.search, options.bands.as_deref(), resolve).await?;
        let mosaic_options = IndexedMosaicOptions::new(options.min_zoom, index.bands().to_vec())
            .with_max_zoom(options.max_zoom)
            .with_nodata(options.nodata)
            .with_max_cached_files(options.max_cached_files)
            .with_file_expiry(options.file_expiry)
            .with_file_idle_timeout(options.file_idle_timeout);
        let mosaic = AsyncIndexedMosaic::open_stac(index, &probe, mosaic_options).await?;
        let info = *mosaic.info();
        let [west, south, east, north] = mosaic.bounds().unwrap_or([-180.0, -MAX_LATITUDE, 180.0, MAX_LATITUDE]);
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
                    user_agent: "martin/test".to_owned(),
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
                    user_agent: "martin/test".to_owned(),
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
    async fn a_next_link_outside_the_api_origin_fails_the_search() {
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
                "links": [{"rel": "next", "href": "http://internal.invalid/admin", "method": "POST", "body": {"drop": "everything"}}],
            })))
            .with_priority(2)
            .mount(&server)
            .await;
        let source = stac_source(&server).await;

        let error = source
            .get_tile(TileCoord::new_unchecked(16, 10_477, 25_326), None)
            .await
            .unwrap_err();

        assert_eq!(
            std::error::Error::source(&error).map(ToString::to_string),
            Some(format!(
                "The STAC API at {} links its next search page to http://internal.invalid/admin, outside its origin",
                server.uri()
            ))
        );
    }

    #[tokio::test]
    async fn an_asset_that_orders_its_bands_unlike_the_first_item_is_skipped() {
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
                "features": [{
                    "type": "Feature",
                    "id": "a",
                    "bbox": [-122.454_75, 37.790_5, -122.437_3, 37.802_2],
                    "properties": {"datetime": "2022-05-18T16:00:00Z"},
                    "assets": {"image": {
                        "href": "memory://a.tif",
                        "eo:bands": [{"common_name": "red"}, {"common_name": "green"}, {"common_name": "blue"}],
                    }},
                }],
            })))
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "features": [{
                    "type": "Feature",
                    "id": "b",
                    "bbox": [-122.454_75, 37.790_5, -122.437_3, 37.802_2],
                    "properties": {"datetime": "2022-05-18T16:00:00Z"},
                    "assets": {"image": {
                        "href": "memory://b.tif",
                        "eo:bands": [{"common_name": "blue"}, {"common_name": "green"}, {"common_name": "red"}],
                    }},
                }],
            })))
            .with_priority(2)
            .mount(&server)
            .await;
        let source = stac_source(&server).await;

        let tile = source
            .get_tile(TileCoord::new_unchecked(16, 10_477, 25_326), None)
            .await
            .unwrap();

        assert!(tile.is_empty());
    }

    #[tokio::test]
    async fn an_item_whose_bbox_crosses_the_date_line_is_searched_at_every_longitude() {
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
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "features": [{
                    "type": "Feature",
                    "id": "a",
                    "bbox": [179.0, 37.790_5, -122.437_3, 37.802_2],
                    "properties": {"datetime": "2022-05-18T16:00:00Z"},
                    "assets": {"image": {"href": "memory://a.tif"}},
                }],
            })))
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
    async fn a_min_zoom_below_the_search_zoom_fails_before_reading_the_api() {
        let server = MockServer::start().await;

        let error = CogSource::new_stac(
            "stac".to_owned(),
            CogStacOptions {
                search: StacSearchOptions {
                    url: server.uri(),
                    user_agent: "martin/test".to_owned(),
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
                min_zoom: 12,
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

        assert_eq!(error.to_string(), "The min zoom 12 is below the search zoom 13");
        assert!(server.received_requests().await.unwrap().is_empty());
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
                    user_agent: "martin/test".to_owned(),
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
