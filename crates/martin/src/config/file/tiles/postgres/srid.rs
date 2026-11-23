//! A `PostGIS` spatial reference id that names a coordinate reference system.

use std::fmt::{self, Display, Formatter};
use std::num::NonZeroU32;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The largest SRID `PostGIS` accepts, its `SRID_MAXIMUM`.
const SRID_MAXIMUM: u32 = 999_999;

/// A `PostGIS` SRID from 1 to 999999, so never SRID 0, which means "no CRS".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "i32", into = "i32")]
pub struct Srid(NonZeroU32);

/// An integer outside the range of SRIDs `PostGIS` accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("SRID {0} is out of range, it must be from 1 to {SRID_MAXIMUM}")]
pub struct SridOutOfRange(pub i32);

impl Srid {
    /// The SRID as `PostGIS` takes it in a query parameter.
    #[must_use]
    pub fn get(self) -> i32 {
        i32::try_from(self.0.get()).expect("an SRID is at most 999999")
    }
}

impl TryFrom<i32> for Srid {
    type Error = SridOutOfRange;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        u32::try_from(value)
            .ok()
            .filter(|srid| *srid <= SRID_MAXIMUM)
            .and_then(NonZeroU32::new)
            .map(Self)
            .ok_or(SridOutOfRange(value))
    }
}

impl From<Srid> for i32 {
    fn from(srid: Srid) -> Self {
        srid.get()
    }
}

impl FromStr for Srid {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value: i32 = s.parse().map_err(|_not_an_integer| format!("{s} is not an integer"))?;
        Self::try_from(value).map_err(|e| e.to_string())
    }
}

impl Display for Srid {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
