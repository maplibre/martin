use std::sync::LazyLock;

use actix_web::http::Uri;

use crate::MartinError::BasePathError;
use crate::{MartinError, MartinResult};

pub fn init_aws_lc_tls() -> MartinResult<()> {
    // https://github.com/rustls/rustls/issues/1877
    static INIT_TLS: LazyLock<Result<(), String>> = LazyLock::new(|| {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .map_err(|e| format!("Unable to init rustls: {e:?}"))
    });
    (*INIT_TLS)
        .clone()
        .map_err(|e| MartinError::InternalError(e.into()))
}

pub fn parse_base_path(path: &str) -> MartinResult<String> {
    if !path.starts_with('/') {
        return Err(BasePathError(path.to_string()));
    }
    if let Ok(uri) = path.parse::<Uri>() {
        return Ok(uri.path().trim_end_matches('/').to_string());
    }
    Err(BasePathError(path.to_string()))
}

#[cfg(test)]
pub mod tests {
    use crate::utils::parse_base_path;
    #[test]
    fn test_parse_base_path() {
        for (path, expected) in [
            ("/", Some("")),
            ("//", Some("")),
            ("/foo/bar", Some("/foo/bar")),
            ("/foo/bar/", Some("/foo/bar")),
            ("", None),
            ("foo/bar", None),
        ] {
            match expected {
                Some(v) => assert_eq!(v, parse_base_path(path).unwrap()),
                None => assert!(parse_base_path(path).is_err()),
            }
        }
    }
}
