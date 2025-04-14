use std::string::ToString;

use actix_web::error::ErrorBadRequest;
use actix_web::http::Uri;
use actix_web::web::{Data, Path};
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, middleware, route};
use itertools::Itertools as _;
use serde::Deserialize;
use tilejson::{TileJSON, tilejson};

use crate::source::{TileInfoSource, TileSources};
use crate::srv::SrvConfig;

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

    let tiles_path = if let Some(base_path) = &srv_config.base_path {
        format!("{base_path}/{}", path.source_ids)
    } else {
        req.headers()
            .get("x-rewrite-url")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<Uri>().ok())
            .map_or_else(|| req.path().to_string(), |v| v.path().to_string())
    };

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

#[must_use]
pub fn merge_tilejson(sources: &[TileInfoSource], tiles_url: String) -> TileJSON {
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

    use tilejson::{Bounds, VectorLayer};

    use super::*;
    use crate::srv::server::tests::TestSource;

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
            on_invalid: None,
            validation_level: None,
            data: Vec::default(),
        };
        let tj = merge_tilejson(&[Box::new(src1.clone())], url.clone());
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
            validation_level: None,
            on_invalid: None,
        };

        let tj = merge_tilejson(&[Box::new(src1.clone()), Box::new(src2)], url.clone());
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
