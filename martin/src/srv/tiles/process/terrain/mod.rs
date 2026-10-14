mod contour;
mod hillshade;
mod neighbourhood;

pub use contour::{ContourTraceError, trace_contour};
pub use hillshade::{HillshadeBakeError, bake_hillshade};
