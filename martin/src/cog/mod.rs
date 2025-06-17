#![doc = include_str!("README.md")]

mod config;
mod errors;
mod image;
mod model;
mod source;

pub use config::CogConfig;
pub use errors::CogError;
