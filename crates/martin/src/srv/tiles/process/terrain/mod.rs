mod contour;
mod hillshade;
mod neighbourhood;

pub use crate::srv::tiles::process::terrain::contour::{ContourTraceError, trace_contour};
pub use crate::srv::tiles::process::terrain::hillshade::{HillshadeBakeError, bake_hillshade};
