//! Proxies tiles from an upstream HTTP tile server through Martin's pipeline.
//!
//! A `passthrough` source fetches tiles from an operator-configured upstream URL (a
//! `{z}/{x}/{y}` template, a list of templates, or a `TileJSON` document URL) and serves the
//! bytes verbatim, preserving the upstream `Content-Encoding`. The shared server pipeline then
//! applies MVT<->MLT conversion and caching on top, exactly as it does for any other [`Source`].
//!
//! [`Source`]: crate::tiles::Source

mod error;
pub use error::PassthroughError;

mod url;
pub use url::UrlTemplate;

mod source;
pub use source::{PassthroughSource, TemplateMeta, TemplateSet, Transport, Upstream};
