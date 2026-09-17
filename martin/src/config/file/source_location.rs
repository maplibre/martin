//! Classification of a configured tile source string into where it lives.

use std::path::{Path, PathBuf};

use url::Url;

use crate::config::file::{ConfigFileError, ConfigFileResult};

/// Where a configured tile source lives.
///
/// Produced by [`SourceLocation::classify`], which owns the scheme table and the URL parsing
/// so that callers never re-parse a source string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceLocation {
    /// A path on the local filesystem, including `SQLite` connection strings such as
    /// `file:name.mbtiles?mode=memory&cache=shared`.
    Local(PathBuf),
    /// An object store URL, resolved by `object_store`.
    ObjectStore(Url),
    /// An `http`/`https` URL.
    Http(Url),
}

impl SourceLocation {
    /// Classify a configured source string.
    ///
    /// Anything that is not a URL with a recognised scheme and an authority (`scheme://`) is a
    /// local path.
    ///
    /// # Errors
    /// Returns [`ConfigFileError::InvalidSourceUrl`] if the string carries a recognised remote
    /// scheme but is not a valid URL.
    pub fn classify(raw: &str) -> ConfigFileResult<Self> {
        if let Some(url) = Self::parse_remote(raw, &["http", "https"])? {
            return Ok(Self::Http(url));
        }
        if let Some(url) = Self::parse_remote(
            raw,
            &[
                "s3", "s3a", "gs", "az", "adl", "azure", "abfs", "abfss", "file",
            ],
        )? {
            return Ok(Self::ObjectStore(url));
        }
        Ok(Self::Local(PathBuf::from(raw)))
    }

    /// Parse `raw` as a URL if it carries one of `schemes` in `scheme://` form, and `None`
    /// otherwise - a local path.
    ///
    /// Callers that reach a store through something other than `object_store` - `DuckDB`, which
    /// knows its own set of schemes - pass their own table and get the same parsing rules.
    ///
    /// # Errors
    /// Returns [`ConfigFileError::InvalidSourceUrl`] if the string carries one of `schemes` but is
    /// not a valid URL.
    pub fn parse_remote(raw: &str, schemes: &[&str]) -> ConfigFileResult<Option<Url>> {
        let Some((scheme, _)) = raw.split_once("://") else {
            return Ok(None);
        };
        if !schemes.contains(&scheme) {
            return Ok(None);
        }
        Url::parse(raw)
            .map(Some)
            .map_err(|e| ConfigFileError::InvalidSourceUrl(e, raw.to_owned()))
    }

    /// Classify a configured source path. Paths that are not valid UTF-8 are always local.
    ///
    /// # Errors
    /// See [`SourceLocation::classify`].
    pub fn classify_path(path: &Path) -> ConfigFileResult<Self> {
        match path.to_str() {
            Some(raw) => Self::classify(raw),
            None => Ok(Self::Local(path.to_path_buf())),
        }
    }

    /// Whether this location is served over the network rather than from the local filesystem.
    #[must_use]
    pub const fn is_remote(&self) -> bool {
        !matches!(self, Self::Local(_))
    }

    /// The URL of a remote location, or `None` for a local path.
    #[must_use]
    pub const fn url(&self) -> Option<&Url> {
        match self {
            Self::Local(_) => None,
            Self::ObjectStore(url) | Self::Http(url) => Some(url),
        }
    }

    /// Consumes the location, yielding the URL of a remote one and `None` for a local path.
    #[must_use]
    pub fn into_url(self) -> Option<Url> {
        match self {
            Self::Local(_) => None,
            Self::ObjectStore(url) | Self::Http(url) => Some(url),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use url::Url;

    use super::*;

    #[rstest]
    #[case::s3("s3")]
    #[case::s3a("s3a")]
    #[case::gs("gs")]
    #[case::az("az")]
    #[case::adl("adl")]
    #[case::azure("azure")]
    #[case::abfs("abfs")]
    #[case::abfss("abfss")]
    #[case::file("file")]
    fn object_store_schemes_are_object_store_urls(#[case] scheme: &str) {
        let raw = format!("{scheme}://bucket/dir/tiles.pmtiles");
        let location = SourceLocation::classify(&raw).expect("classification should succeed");
        assert_eq!(
            location,
            SourceLocation::ObjectStore(raw.parse().expect("valid url"))
        );
        assert!(location.is_remote());
    }

    #[rstest]
    #[case::http("http")]
    #[case::https("https")]
    fn http_schemes_are_http_urls(#[case] scheme: &str) {
        let raw = format!("{scheme}://example.org/dir/tiles.pmtiles");
        let location = SourceLocation::classify(&raw).expect("classification should succeed");
        assert_eq!(
            location,
            SourceLocation::Http(raw.parse().expect("valid url"))
        );
        assert!(location.is_remote());
    }

    #[rstest]
    #[case::empty("")]
    #[case::bare_filename("tiles.pmtiles")]
    #[case::relative("relative/dir/tiles.mbtiles")]
    #[case::absolute("/var/lib/martin/tiles.mbtiles")]
    #[case::windows(r"C:\Users\martin\tiles.pmtiles")]
    #[case::sqlite_memory("file:tiles.mbtiles?mode=memory&cache=shared")]
    #[case::file_without_authority("file:/var/lib/martin/tiles.mbtiles")]
    #[case::s3_without_authority("s3:bucket/tiles.pmtiles")]
    #[case::uppercase_scheme("S3://bucket/tiles.pmtiles")]
    #[case::unsupported_scheme("ftp://example.org/tiles.pmtiles")]
    #[case::postgres("postgresql://localhost/db")]
    #[case::separator_inside_path("/var/lib/weird://name.mbtiles")]
    fn strings_without_a_recognised_remote_scheme_are_local(#[case] raw: &str) {
        let location = SourceLocation::classify(raw).expect("classification should succeed");
        assert_eq!(location, SourceLocation::Local(raw.into()));
        assert!(!location.is_remote());
        assert_eq!(location.url(), None);
    }

    #[rstest]
    #[case::empty_http_host("http://")]
    #[case::empty_https_host("https://")]
    #[case::unterminated_ipv6("http://[::1")]
    fn a_remote_scheme_that_is_not_a_url_is_an_error(#[case] raw: &str) {
        SourceLocation::classify(raw).unwrap_err();
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_are_local() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt as _;
        use std::path::Path;

        let path = Path::new(OsStr::from_bytes(b"/var/lib/\xff.mbtiles"));
        assert_eq!(
            SourceLocation::classify_path(path).expect("classification should succeed"),
            SourceLocation::Local(path.to_path_buf())
        );
    }

    #[test]
    fn remote_locations_yield_their_url() {
        let location = SourceLocation::classify("s3://bucket/tiles.pmtiles")
            .expect("classification should succeed");
        assert_eq!(
            location.url().map(Url::as_str),
            Some("s3://bucket/tiles.pmtiles")
        );
        assert_eq!(
            location.into_url().map(|url| url.to_string()),
            Some("s3://bucket/tiles.pmtiles".to_owned())
        );
    }

    #[rstest]
    #[case::in_the_table(
        "hf://datasets/org/set/data.parquet",
        Some("hf://datasets/org/set/data.parquet")
    )]
    #[case::not_in_the_table("s3://bucket/data.parquet", None)]
    #[case::without_authority("hf:datasets/org/set/data.parquet", None)]
    fn a_caller_table_parses_the_schemes_it_lists_and_no_others(
        #[case] raw: &str,
        #[case] expected: Option<&str>,
    ) {
        let url = SourceLocation::parse_remote(raw, &["hf"]).expect("parsing should succeed");
        assert_eq!(url.as_ref().map(Url::as_str), expected);
    }

    #[test]
    fn local_locations_yield_no_url() {
        let location =
            SourceLocation::classify("tiles.pmtiles").expect("classification should succeed");
        assert_eq!(location.into_url(), None);
    }
}
