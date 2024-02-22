use std::string::ToString;

use actix_web::error::ErrorBadRequest;
use actix_web::http::Uri;
use actix_web::web::{Data, Path};
use actix_web::{middleware, route, HttpRequest, HttpResponse, Result as ActixResult};
use itertools::Itertools as _;
use serde::Deserialize;
use tilejson::{tilejson, TileJSON};

use crate::source::{Source, TileSources};
use crate::srv::SrvConfig;
use crate::utils::parse_base_path;
#[derive(Deserialize)]
pub struct SourceIDsRequest {
    pub source_ids: String,
}

#[route(
    "/{source_ids}",
    method = "GET",
    method = "HEAD",
    wrap = "middleware::Compress::default()"
)]
#[allow(clippy::unused_async)]
async fn get_source_info(
    req: HttpRequest,
    path: Path<SourceIDsRequest>,
    sources: Data<TileSources>,
    srv_config: Data<SrvConfig>,
) -> ActixResult<HttpResponse> {
    let sources = sources.get_sources(&path.source_ids, None)?.0;

    let tiles_path = generate_tiles_base(&srv_config, &req, &path);

    let query_string = req.query_string();
    let path_and_query = if query_string.is_empty() {
        format!("{tiles_path}/{{z}}/{{x}}/{{y}}")
    } else {
        format!("{tiles_path}/{{z}}/{{x}}/{{y}}?{query_string}")
    };

    // Construct a tiles URL from the request info, including the query string if present.
    let info = req.connection_info();
    let tiles_url = Uri::builder()
        .scheme(info.scheme())
        .authority(info.host())
        .path_and_query(path_and_query)
        .build()
        .map(|tiles_url| tiles_url.to_string())
        .map_err(|e| ErrorBadRequest(format!("Can't build tiles URL: {e}")))?;

    Ok(HttpResponse::Ok().json(merge_tilejson(&sources, tiles_url)))
}

fn generate_tiles_base(
    srv_config: &SrvConfig,
    req: &HttpRequest,
    path: &Path<SourceIDsRequest>,
) -> String {
    let mut result = None;
    if let Some(path_from_config) = &srv_config.base_path {
        let base = format!("{path_from_config}/{0}", &path.source_ids);
        result = parse_base_path(&base).ok();
    } else if let Some(rewrite_url) = req.headers().get("x-rewrite-url") {
        if let Ok(url_str) = rewrite_url.to_str() {
            result = parse_base_path(&url_str.to_string()).ok();
        } else {
            result = None;
        }
    }
    if let Some(base_path) = result {
        base_path
    } else {
        req.path().to_owned()
    }
}

#[must_use]
pub fn merge_tilejson(sources: &[&dyn Source], tiles_url: String) -> TileJSON {
    if sources.len() == 1 {
        let mut tj = sources[0].get_tilejson().clone();
        tj.tiles = vec![tiles_url];
        return tj;
    }

    let mut attributions = vec![];
    let mut descriptions = vec![];
    let mut names = vec![];
    let mut result = tilejson! {
        tiles: vec![tiles_url],
    };

    for src in sources {
        let tj = src.get_tilejson();

        if let Some(vector_layers) = &tj.vector_layers {
            if let Some(ref mut a) = result.vector_layers {
                a.extend(vector_layers.iter().cloned());
            } else {
                result.vector_layers = Some(vector_layers.clone());
            }
        }

        if let Some(v) = &tj.attribution {
            if !attributions.contains(&v) {
                attributions.push(v);
            }
        }

        if let Some(bounds) = tj.bounds {
            if let Some(a) = result.bounds {
                result.bounds = Some(a + bounds);
            } else {
                result.bounds = tj.bounds;
            }
        }

        if result.center.is_none() {
            // Use first found center. Averaging multiple centers might create a center in the middle of nowhere.
            result.center = tj.center;
        }

        if let Some(v) = &tj.description {
            if !descriptions.contains(&v) {
                descriptions.push(v);
            }
        }

        if let Some(maxzoom) = tj.maxzoom {
            if let Some(a) = result.maxzoom {
                if a < maxzoom {
                    result.maxzoom = tj.maxzoom;
                }
            } else {
                result.maxzoom = tj.maxzoom;
            }
        }

        if let Some(minzoom) = tj.minzoom {
            if let Some(a) = result.minzoom {
                if a > minzoom {
                    result.minzoom = tj.minzoom;
                }
            } else {
                result.minzoom = tj.minzoom;
            }
        }

        if let Some(name) = &tj.name {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }

    if !attributions.is_empty() {
        result.attribution = Some(attributions.into_iter().join("\n"));
    }

    if !descriptions.is_empty() {
        result.description = Some(descriptions.into_iter().join("\n"));
    }

    if !names.is_empty() {
        result.name = Some(names.into_iter().join(","));
    }

    result
}

#[cfg(test)]
pub mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::srv::server::tests::TestSource;
    use actix_web::test::TestRequest;
    use tilejson::{Bounds, VectorLayer};

    #[actix_web::test]
    #[test]
    async fn test_generate_tiles_base_path() {
        // With config but rewrite url
        let srv_config = SrvConfig {
            base_path: Some("/tiles".to_string()),
            ..Default::default()
        };
        let req = TestRequest::default().uri("/MixedPoints").to_http_request();
        let path = Path::from(SourceIDsRequest {
            source_ids: "MixedPoints".to_string(),
        });

        let base = generate_tiles_base(&srv_config, &req, &path);
        assert_eq!(base, "/tiles/MixedPoints");

        // With rewrite url but config
        let srv_config = SrvConfig::default();
        let req = TestRequest::default()
            .insert_header(("x-rewrite-url", "/tiles/MixedPoints"))
            .to_http_request();
        let path = Path::from(SourceIDsRequest {
            source_ids: "MixedPoints".to_string(),
        });

        let base = generate_tiles_base(&srv_config, &req, &path);
        assert_eq!(base, "/tiles/MixedPoints");

        // no config, no rewrite url
        let srv_config = SrvConfig::default();
        let req = TestRequest::default().uri("/MixedPoints").to_http_request();
        let path = Path::from(SourceIDsRequest {
            source_ids: "MixedPoints".to_string(),
        });

        let base = generate_tiles_base(&srv_config, &req, &path);
        assert_eq!(base, "/MixedPoints");

        // both
        let srv_config = SrvConfig {
            base_path: Some("/foo".to_string()),
            ..Default::default()
        };
        let req = TestRequest::default()
            .insert_header(("x-rewrite-url", "/bar"))
            .to_http_request();
        let path = Path::from(SourceIDsRequest {
            source_ids: "MixedPoints".to_string(),
        });

        let base = generate_tiles_base(&srv_config, &req, &path);
        assert_eq!(base, "/foo/MixedPoints");

        // todo  other edge cases
    }

    #[test]
    fn test_merge_tilejson() {
        let url = "http://localhost:8888/foo/{z}/{x}/{y}".to_string();
        let src1 = TestSource {
            id: "id",
            tj: tilejson! {
                tiles: vec![],
                name: "layer1".to_string(),
                minzoom: 5,
                maxzoom: 10,
                bounds: Bounds::new(-10.0, -20.0, 10.0, 20.0),
                vector_layers: vec![
                    VectorLayer::new("layer1".to_string(),
                    BTreeMap::from([
                        ("a".to_string(), "x1".to_string()),
                    ]))
                ],
            },
            data: Vec::default(),
        };
        let tj = merge_tilejson(&[&src1], url.clone());
        assert_eq!(
            TileJSON {
                tiles: vec![url.clone()],
                ..src1.tj.clone()
            },
            tj
        );

        let src2 = TestSource {
            id: "id",
            tj: tilejson! {
                tiles: vec![],
                name: "layer2".to_string(),
                minzoom: 7,
                maxzoom: 12,
                bounds: Bounds::new(-20.0, -5.0, 5.0, 50.0),
                vector_layers: vec![
                    VectorLayer::new("layer2".to_string(),
                    BTreeMap::from([
                        ("b".to_string(), "x2".to_string()),
                    ]))
                ],
            },
            data: Vec::default(),
        };

        let tj = merge_tilejson(&[&src1, &src2], url.clone());
        assert_eq!(tj.tiles, vec![url]);
        assert_eq!(tj.name, Some("layer1,layer2".to_string()));
        assert_eq!(tj.minzoom, Some(5));
        assert_eq!(tj.maxzoom, Some(12));
        assert_eq!(tj.bounds, Some(Bounds::new(-20.0, -20.0, 10.0, 50.0)));
        assert_eq!(
            tj.vector_layers,
            Some(vec![
                VectorLayer::new(
                    "layer1".to_string(),
                    BTreeMap::from([("a".to_string(), "x1".to_string())])
                ),
                VectorLayer::new(
                    "layer2".to_string(),
                    BTreeMap::from([("b".to_string(), "x2".to_string())])
                ),
            ])
        );
    }
}
