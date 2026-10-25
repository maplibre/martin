use std::fmt::Write as _;

use actix_web::HttpRequest;
use serde::Serialize;

use crate::config::file::srv::SrvConfig;
use crate::srv::server::path_prefix;

/// A link from one OGC API resource to another.
#[derive(Serialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
pub struct Link {
    pub href: String,
    pub rel: &'static str,
    #[serde(rename = "type")]
    pub media_type: &'static str,
    pub title: &'static str,
    /// Whether `href` is a URI template.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub templated: bool,
}

impl Link {
    /// A link to a JSON resource.
    pub fn json(href: String, rel: &'static str, title: &'static str) -> Self {
        Self {
            href,
            rel,
            media_type: "application/json",
            title,
            templated: false,
        }
    }
}

/// `segment` as one URL path segment, with everything but the unreserved characters of RFC 3986 percent-encoded.
pub fn path_segment(segment: &str) -> String {
    let mut encoded = String::new();
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    encoded
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
