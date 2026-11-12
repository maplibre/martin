//! `limit`: keeps the first features of a layer in a tile.

use super::TileFeature;

/// Keeps the first `count` of `features`.
pub(super) fn apply(count: u32, features: &mut Vec<TileFeature>) {
    features.truncate(usize::try_from(count).unwrap_or(usize::MAX));
}
