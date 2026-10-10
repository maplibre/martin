use std::fmt;

use martin_tile_utils::MAX_ZOOM;
use serde::{Deserialize, Deserializer, Serialize, de};

use super::error::TilingConfigError;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Zoom(u8);

impl Zoom {
    #[must_use]
    pub fn new(zoom: u8) -> Option<Self> {
        (zoom <= MAX_ZOOM).then_some(Self(zoom))
    }

    #[must_use]
    pub fn get(self) -> u8 {
        self.0
    }

    pub(super) fn out_of_range<E: de::Error>(unexpected: de::Unexpected<'_>) -> E {
        E::invalid_value(unexpected, &format!("a zoom from 0 to {MAX_ZOOM}").as_str())
    }
}

impl fmt::Debug for Zoom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "z{}", self.0)
    }
}

impl<'de> Deserialize<'de> for Zoom {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let zoom = u8::deserialize(deserializer)?;
        Self::new(zoom).ok_or_else(|| Self::out_of_range(de::Unexpected::Unsigned(zoom.into())))
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
pub struct ZoomRange {
    minzoom: Option<Zoom>,
    maxzoom: Option<Zoom>,
}

impl fmt::Debug for ZoomRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.minzoom, self.maxzoom) {
            (None, None) => f.write_str("all zooms"),
            (Some(min), None) => write!(f, "{min:?}.."),
            (None, Some(max)) => write!(f, "..={max:?}"),
            (Some(min), Some(max)) => write!(f, "{min:?}..={max:?}"),
        }
    }
}

impl ZoomRange {
    pub fn new(minzoom: Option<Zoom>, maxzoom: Option<Zoom>) -> Result<Self, TilingConfigError> {
        match (minzoom, maxzoom) {
            (Some(min), Some(max)) if min > max => Err(TilingConfigError::MinzoomAboveMaxzoom {
                min: min.get(),
                max: max.get(),
            }),
            _ => Ok(Self { minzoom, maxzoom }),
        }
    }

    #[must_use]
    pub fn minzoom(self) -> Option<Zoom> {
        self.minzoom
    }

    #[must_use]
    pub fn maxzoom(self) -> Option<Zoom> {
        self.maxzoom
    }

    #[must_use]
    pub fn is_unbounded(self) -> bool {
        self == Self::default()
    }
}
