use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

/// Size of a rendered XYZ tile in logical pixels, before the pixel ratio.
///
/// Most raster tile clients expect 256 px tiles; `MapLibre` zoom levels are based on 512 px ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub enum TileSize {
    /// 256 px tiles.
    Px256,
    /// 512 px tiles.
    #[default]
    Px512,
}

/// A tile size other than 256 or 512.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("tile size must be 256 or 512, got {0}")]
pub struct InvalidTileSize(pub u32);

impl TileSize {
    /// Width and height of the tile in logical pixels.
    #[must_use]
    pub const fn px(self) -> NonZeroU32 {
        match self {
            Self::Px256 => NonZeroU32::new(256).expect("256 is non-zero"),
            Self::Px512 => NonZeroU32::new(512).expect("512 is non-zero"),
        }
    }
}

impl TryFrom<u32> for TileSize {
    type Error = InvalidTileSize;

    fn try_from(px: u32) -> Result<Self, Self::Error> {
        match px {
            256 => Ok(Self::Px256),
            512 => Ok(Self::Px512),
            other => Err(InvalidTileSize(other)),
        }
    }
}

impl From<TileSize> for u32 {
    fn from(size: TileSize) -> Self {
        size.px().get()
    }
}

#[cfg(feature = "unstable-schemas")]
impl schemars::JsonSchema for TileSize {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("TileSize")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "Size of a rendered XYZ tile in logical pixels.",
            "type": "integer",
            "enum": [256, 512]
        })
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::px256(256, TileSize::Px256)]
    #[case::px512(512, TileSize::Px512)]
    fn a_supported_size_converts(#[case] px: u32, #[case] size: TileSize) {
        assert_eq!(TileSize::try_from(px), Ok(size));
        assert_eq!(u32::from(size), px);
    }

    #[rstest]
    #[case::zero(0)]
    #[case::between(300)]
    #[case::retina(1024)]
    fn an_unsupported_size_is_rejected(#[case] px: u32) {
        assert_eq!(TileSize::try_from(px), Err(InvalidTileSize(px)));
    }

    #[test]
    fn the_default_is_the_maplibre_tile_size() {
        assert_eq!(TileSize::default(), TileSize::Px512);
    }
}
