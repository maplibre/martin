//! Where a COG or a mosaic is: a local path, or a URL of an object store.

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex, PoisonError};

use cog_tiles::HrefResolver;
use object_store::aws::AmazonS3Builder;
use object_store::azure::MicrosoftAzureBuilder;
use object_store::gcp::GoogleCloudStorageBuilder;
use object_store::http::HttpBuilder;
use object_store::local::LocalFileSystem;
use object_store::path::Path;
use object_store::{ObjectStore, ObjectStoreScheme};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use url::Url;

/// A local path or an object in a store.
pub enum Location {
    Local(PathBuf),
    Store {
        store: Arc<dyn ObjectStore>,
        path: Path,
        /// The URL without credentials, query or fragment, for errors.
        display: String,
    },
}

impl Location {
    /// Parses a local path, a `file://` URL, or an `s3://`, `gs://`, `az://` or `http(s)://` URL.
    ///
    /// Object store credentials and settings come from the usual environment variables
    /// (e.g. `AWS_REGION`, `GOOGLE_SERVICE_ACCOUNT`), overridden by `options`
    /// (e.g. `{"aws_skip_signature": "true"}`).
    pub fn parse(location: &str, options: &[(String, String)]) -> PyResult<Self> {
        match as_url(location) {
            Some(url) => Self::parse_url(&url, options).map_err(|e| PyValueError::new_err(format!("{location}: {e}"))),
            None => Ok(Self::Local(PathBuf::from(location))),
        }
    }

    /// Parses a `file://`, `s3://`, `gs://`, `az://` or `http(s)://` URL, as [`parse`](Self::parse) does.
    fn parse_url(url: &Url, options: &[(String, String)]) -> Result<Self, String> {
        let (scheme, path) = ObjectStoreScheme::parse(url).map_err(|e| e.to_string())?;
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "new schemes are unsupported until added here"
        )]
        let store: Arc<dyn ObjectStore> = match scheme {
            ObjectStoreScheme::Local => {
                let path = url.to_file_path().map_err(|()| "not a local path".to_owned())?;
                return Ok(Self::Local(path));
            }
            ObjectStoreScheme::AmazonS3 => {
                let mut builder = AmazonS3Builder::from_env().with_url(url.as_str());
                for (key, value) in options {
                    builder = builder.with_config(config_key(key)?, value);
                }
                Arc::new(builder.build().map_err(|e| e.to_string())?)
            }
            ObjectStoreScheme::GoogleCloudStorage => {
                let mut builder = GoogleCloudStorageBuilder::from_env().with_url(url.as_str());
                for (key, value) in options {
                    builder = builder.with_config(config_key(key)?, value);
                }
                Arc::new(builder.build().map_err(|e| e.to_string())?)
            }
            ObjectStoreScheme::MicrosoftAzure => {
                let mut builder = MicrosoftAzureBuilder::from_env().with_url(url.as_str());
                for (key, value) in options {
                    builder = builder.with_config(config_key(key)?, value);
                }
                Arc::new(builder.build().map_err(|e| e.to_string())?)
            }
            ObjectStoreScheme::Http => {
                let mut builder = HttpBuilder::new().with_url(&url[..url::Position::BeforePath]);
                for (key, value) in options {
                    builder = builder.with_config(config_key(key)?, value);
                }
                Arc::new(builder.build().map_err(|e| e.to_string())?)
            }
            other => return Err(format!("{other:?} URLs are not supported")),
        };
        Ok(Self::Store {
            store,
            path,
            display: without_secrets(url),
        })
    }
}

/// Resolves the file URLs of an index to stores built as [`Location::parse`] builds them, one per
/// host and first path segment, which holds the bucket or container of path-style URLs.
/// A URL with a query (e.g. a presigned URL) gets a store of its own.
pub fn href_resolver(options: Vec<(String, String)>) -> HrefResolver {
    let local: Arc<dyn ObjectStore> = Arc::new(LocalFileSystem::new());
    let stores: Mutex<HashMap<String, Arc<dyn ObjectStore>>> = Mutex::default();
    Arc::new(move |href: &str| {
        let url = Url::parse(href)?;
        let first_segment = url.path_segments().and_then(|mut s| s.next()).unwrap_or_default();
        let key = format!("{}/{first_segment}", &url[..url::Position::BeforePath]);
        let cached = url.query().is_none().then(|| {
            let stores = stores.lock().unwrap_or_else(PoisonError::into_inner);
            stores.get(&key).cloned()
        });
        if let Some(Some(store)) = cached {
            let (_, path) = ObjectStoreScheme::parse(&url)?;
            return Ok((store, path));
        }
        match Location::parse_url(&url, &options)? {
            Location::Local(path) => Ok((Arc::clone(&local), Path::from_absolute_path(path)?)),
            Location::Store { store, path, .. } => {
                if url.query().is_none() {
                    let mut stores = stores.lock().unwrap_or_else(PoisonError::into_inner);
                    stores.insert(key, Arc::clone(&store));
                }
                Ok((store, path))
            }
        }
    })
}

/// Splits a mosaic location into the location of its files and the file name template under it,
/// at the last `/` before the first placeholder. A URL query stays with the location.
pub fn split_mosaic(location: &str) -> (String, &str) {
    let (path, query) = location
        .split_once('?')
        .map_or((location, None), |(path, query)| (path, Some(query)));
    let before_placeholder = path.split('{').next().unwrap_or_default();
    let split = before_placeholder.rfind(['/', '\\']).map_or(0, |i| i + 1);
    let (root, template) = path.split_at(split);
    let root = match (root, query) {
        ("", None) => ".".to_owned(),
        (root, None) => root.to_owned(),
        (root, Some(query)) => format!("{root}?{query}"),
    };
    (root, template)
}

/// `location` as a URL, if it has a scheme other than a Windows drive letter.
fn as_url(location: &str) -> Option<Url> {
    let url = Url::parse(location).ok()?;
    (url.scheme().len() > 1).then_some(url)
}

fn config_key<K: FromStr<Err = object_store::Error>>(key: &str) -> Result<K, String> {
    key.parse().map_err(|e| format!("unknown option {key:?}: {e}"))
}

fn without_secrets(url: &Url) -> String {
    let mut url = url.clone();
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}
