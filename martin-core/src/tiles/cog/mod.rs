#![doc = include_str!("README.md")]

mod errors;
mod image;
mod model;
mod reader;
mod source;

pub use errors::CogError;
pub use reader::{CogObjectMeta, CogReader, CogReaderError, ObjectStoreCogReader};
pub use source::CogSource;
