use actix_web::HttpRequest;
use serde::Serialize;

use crate::config::file::srv::SrvConfig;
use crate::srv::server::path_prefix;

/// A link from one OGC API resource to another.
#[derive(Serialize, Debug)]
pub struct Link {
    pub href: String,
    pub rel: &'static str,
    #[serde(rename = "type")]
    pub media_type: &'static str,
    pub title: &'static str,
}

impl Link {
    /// A link to a JSON resource.
    pub fn json(href: String, rel: &'static str, title: &'static str) -> Self {
        Self {
            href,
            rel,
            media_type: "application/json",
            title,
        }
    }
}

/// The absolute URL of `path` under the OGC API, e.g. `/conformance`.
pub fn ogc_url(req: &HttpRequest, srv_config: &SrvConfig, path: &str) -> String {
    let info = req.connection_info();
    format!(
        "{}://{}{}/ogc{path}",
        info.scheme(),
        info.host(),
        path_prefix(req, srv_config)
    )
}
