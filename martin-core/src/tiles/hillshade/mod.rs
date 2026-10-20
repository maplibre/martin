//! bakes an L8 grayscale relief image from Mapzen *normal* tiles.

mod canvas;
mod encode;
mod error;
mod light;
mod shade;

pub use crate::tiles::hillshade::encode::SUPPORTED_FORMATS;
pub use crate::tiles::hillshade::error::HillshadeError;
pub use crate::tiles::hillshade::light::LightAngles;
pub use crate::tiles::hillshade::shade::{BakeParams, BakedTile, Canvas, bake_with_light, output_apron};
