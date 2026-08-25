//! bakes an L8 grayscale relief image from Mapzen *normal* tiles.

mod canvas;
mod encode;
mod error;
mod light;
mod shade;

pub use encode::SUPPORTED_FORMATS;
pub use error::HillshadeError;
pub use light::LightAngles;
pub use shade::{BakeParams, BakedTile, Canvas, bake_with_light, output_apron};
