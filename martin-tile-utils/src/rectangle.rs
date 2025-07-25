//! Rectangle utilities for working with tile coordinate rectangles.
//!
//! This module provides the `TileRect` struct for representing rectangular regions
//! in tile coordinate space, along with utilities for managing collections of
//! non-overlapping rectangles.

use serde::Serialize;

/// A rectangular region in tile coordinate space.
///
/// Represents a rectangle defined by zoom level and tile coordinates.
/// The rectangle is inclusive of both min and max coordinates.
/// Use [`append_rect`] to merge rectangles without overlapping.
///
/// # Examples
///
/// ```
/// # use martin_tile_utils::TileRect;
/// let rect = TileRect::new(10, 0, 0, 255, 255);
/// assert_eq!(rect.size(), 256 * 256);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileRect {
    /// The zoom level of the tiles
    pub zoom: u8,
    /// The minimum X coordinate (inclusive)
    pub min_x: u32,
    /// The minimum Y coordinate (inclusive)
    pub min_y: u32,
    /// The maximum X coordinate (inclusive)
    pub max_x: u32,
    /// The maximum Y coordinate (inclusive)
    pub max_y: u32,
}

impl Serialize for TileRect {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.collect_str(&format!(
            "{}: ({},{}) - ({},{})",
            self.zoom, self.min_x, self.min_y, self.max_x, self.max_y
        ))
    }
}

impl TileRect {
    /// Creates a new `TileRect` with the specified coordinates.
    ///
    /// # Arguments
    ///
    /// * `zoom` - The zoom level
    /// * `min_x` - The minimum X coordinate (inclusive)
    /// * `min_y` - The minimum Y coordinate (inclusive)
    /// * `max_x` - The maximum X coordinate (inclusive)
    /// * `max_y` - The maximum Y coordinate (inclusive)
    ///
    /// # Panics
    ///
    /// Panics if `min_x > max_x` or `min_y > max_y`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use martin_tile_utils::TileRect;
    /// let rect = TileRect::new(0, 0, 0, 1, 1);
    /// assert_eq!(rect.size(), 4);
    /// ```
    #[must_use]
    pub fn new(zoom: u8, min_x: u32, min_y: u32, max_x: u32, max_y: u32) -> Self {
        assert!(min_x <= max_x);
        assert!(min_y <= max_y);
        TileRect {
            zoom,
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Checks if two rectangles overlap.
    ///
    /// Two rectangles overlap if
    /// - they share the same zoom level and
    /// - their coordinate ranges intersect in both X and Y dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// # use martin_tile_utils::TileRect;
    /// let rect1 = TileRect::new(0, 0, 0, 1, 1);
    /// let rect2 = TileRect::new(0, 1, 1, 2, 2);
    /// assert!(rect1.is_overlapping(&rect2));
    ///
    /// let rect3 = TileRect::new(0, 2, 2, 3, 3);
    /// assert!(!rect1.is_overlapping(&rect3));
    /// ```
    #[must_use]
    pub fn is_overlapping(&self, other: &Self) -> bool {
        self.zoom == other.zoom
            && self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    /// Total number of tiles contained in this rectangle.
    ///
    /// The size is calculated as `(max_x - min_x + 1) * (max_y - min_y + 1)`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use martin_tile_utils::TileRect;
    /// // x = 0..=2 => 3 tiles
    /// // y = 0..=3 => 4 tiles
    /// let rect = TileRect::new(0, 0, 0, 2, 3);
    /// assert_eq!(rect.size(), 3 * 4);
    /// ```
    #[must_use]
    pub fn size(&self) -> u64 {
        u64::from(self.max_x - self.min_x + 1) * u64::from(self.max_y - self.min_y + 1)
    }

    /// Returns up to 4 non-overlapping rectangles that represent the parts of `o`
    /// that do not overlap with `self`.
    ///
    /// This method splits the `o` rectangle into up to 4 parts that extend
    /// beyond the boundaries of `self`. The parts are: left, right, top, and bottom.
    /// The `o` rectangle has to be of the same zoom level.
    fn get_non_overlapping(&self, o: &Self) -> [Option<Self>; 4] {
        let mut result = [None, None, None, None];
        assert_eq!(self.zoom, o.zoom);
        if o.min_x < self.min_x {
            // take the left part of the other rect, entire height
            let min_x = self.min_x - 1;
            result[0] = Some(TileRect::new(o.zoom, o.min_x, o.min_y, min_x, o.max_y));
        }
        if o.max_x > self.max_x {
            // take the right part of the other rect, entire height
            let max_x = self.max_x + 1;
            result[1] = Some(TileRect::new(o.zoom, max_x, o.min_y, o.max_x, o.max_y));
        }
        if o.min_y < self.min_y {
            // take the top part of the other rect, width of self
            let min_x = o.min_x.max(self.min_x);
            let max_x = o.max_x.min(self.max_x);
            result[2] = Some(TileRect::new(o.zoom, min_x, o.min_y, max_x, self.min_y - 1));
        }
        if o.max_y > self.max_y {
            // take the bottom part of the other rect, width of self
            let min_x = o.min_x.max(self.min_x);
            let max_x = o.max_x.min(self.max_x);
            result[3] = Some(TileRect::new(o.zoom, min_x, self.max_y + 1, max_x, o.max_y));
        }
        result
    }
}

/// Appends a new rectangle to a list of rectangles, ensuring no overlaps exist.
///
/// If the new rectangle overlaps with any existing rectangle, it will be split
/// into non-overlapping parts that extend beyond the existing rectangle.
/// This process is recursive and ensures that the final list contains only
/// non-overlapping rectangles.
///
/// # Examples
///
/// ```
/// # use martin_tile_utils::{TileRect, append_rect};
/// let mut rectangles = Vec::new();
/// append_rect(&mut rectangles, TileRect::new(0, 0, 0, 1, 1));
/// append_rect(&mut rectangles, TileRect::new(0, 1, 1, 2, 2));
///
/// // The second rectangle overlaps with the first, so it gets split
/// assert_eq!(rectangles.len(), 3);
/// ```
pub fn append_rect(rectangles: &mut Vec<TileRect>, new_rect: TileRect) {
    for rect in rectangles.iter() {
        if rect.is_overlapping(&new_rect) {
            // add four new non-overlapping rectangles that exceed the existing one
            for new_rect in rect.get_non_overlapping(&new_rect).into_iter().flatten() {
                append_rect(rectangles, new_rect);
            }
            // new rectangle was split into zero to four non-overlapping rectangles and added
            return;
        }
    }
    // no overlap, add the new rectangle
    rectangles.push(new_rect);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn append(rectangles: &mut Vec<TileRect>, new_rect: TileRect) {
        append_rect(rectangles, new_rect);
        // make sure none of the rectangles overlap
        for (i, r1) in rectangles.iter().enumerate() {
            for (j, r2) in rectangles.iter().enumerate() {
                if i != j {
                    assert!(!r1.is_overlapping(r2));
                }
            }
        }
    }

    #[test]
    fn test_len() {
        assert_eq!(1, TileRect::new(0, 0, 0, 0, 0).size());
        assert_eq!(4, TileRect::new(0, 0, 0, 1, 1).size());
        assert_eq!(15, TileRect::new(0, 2, 3, 4, 7).size());
    }

    #[test]
    fn test_tile_range_is_overlapping() {
        let r1 = TileRect::new(0, 0, 0, 0, 0);
        let r2 = TileRect::new(0, 0, 0, 0, 0);
        assert!(r1.is_overlapping(&r2));

        let r1 = TileRect::new(0, 0, 0, 0, 0);
        let r2 = TileRect::new(0, 1, 1, 1, 1);
        assert!(!r1.is_overlapping(&r2));

        let r1 = TileRect::new(0, 0, 0, 1, 1);
        let r2 = TileRect::new(0, 1, 1, 2, 2);
        assert!(r1.is_overlapping(&r2));

        let r1 = TileRect::new(0, 0, 0, 2, 2);
        let r2 = TileRect::new(0, 1, 1, 1, 1);
        assert!(r1.is_overlapping(&r2));

        let center = TileRect::new(0, 4, 4, 6, 6);

        assert!(center.is_overlapping(&TileRect::new(0, 3, 5, 5, 5)));
        assert!(center.is_overlapping(&TileRect::new(0, 5, 3, 5, 5)));
        assert!(center.is_overlapping(&TileRect::new(0, 5, 5, 7, 5)));
        assert!(center.is_overlapping(&TileRect::new(0, 5, 5, 5, 7)));

        assert!(!center.is_overlapping(&TileRect::new(0, 3, 5, 3, 5)));
        assert!(!center.is_overlapping(&TileRect::new(0, 5, 3, 5, 3)));
        assert!(!center.is_overlapping(&TileRect::new(0, 7, 5, 7, 5)));
        assert!(!center.is_overlapping(&TileRect::new(0, 5, 7, 5, 7)));
    }

    #[test]
    fn test_append_single() {
        let mut rectangles = Vec::new();
        append(&mut rectangles, TileRect::new(0, 0, 0, 0, 0));
        assert_eq!(rectangles, vec![TileRect::new(0, 0, 0, 0, 0)]);

        append(&mut rectangles, TileRect::new(0, 0, 0, 0, 0));
        assert_eq!(rectangles, vec![TileRect::new(0, 0, 0, 0, 0)]);

        append(&mut rectangles, TileRect::new(0, 1, 0, 1, 1));
        assert_eq!(
            rectangles,
            vec![TileRect::new(0, 0, 0, 0, 0), TileRect::new(0, 1, 0, 1, 1),]
        );

        append(&mut rectangles, TileRect::new(0, 0, 0, 1, 1));
        assert_eq!(
            rectangles,
            vec![
                TileRect::new(0, 0, 0, 0, 0),
                TileRect::new(0, 1, 0, 1, 1),
                TileRect::new(0, 0, 1, 0, 1)
            ]
        );
    }

    #[test]
    fn test_append_multiple() {
        let mut rectangles = Vec::new();
        append(&mut rectangles, TileRect::new(0, 2, 2, 4, 4));
        assert_eq!(rectangles, vec![TileRect::new(0, 2, 2, 4, 4)]);

        append(&mut rectangles, TileRect::new(0, 1, 3, 3, 3));
        assert_eq!(
            rectangles,
            vec![TileRect::new(0, 2, 2, 4, 4), TileRect::new(0, 1, 3, 1, 3),]
        );

        append(&mut rectangles, TileRect::new(0, 3, 1, 3, 3));
        assert_eq!(
            rectangles,
            vec![
                TileRect::new(0, 2, 2, 4, 4),
                TileRect::new(0, 1, 3, 1, 3),
                TileRect::new(0, 3, 1, 3, 1),
            ]
        );

        append(&mut rectangles, TileRect::new(0, 3, 3, 5, 3));
        assert_eq!(
            rectangles,
            vec![
                TileRect::new(0, 2, 2, 4, 4),
                TileRect::new(0, 1, 3, 1, 3),
                TileRect::new(0, 3, 1, 3, 1),
                TileRect::new(0, 5, 3, 5, 3),
            ]
        );

        append(&mut rectangles, TileRect::new(0, 3, 3, 3, 5));
        assert_eq!(
            rectangles,
            vec![
                TileRect::new(0, 2, 2, 4, 4),
                TileRect::new(0, 1, 3, 1, 3),
                TileRect::new(0, 3, 1, 3, 1),
                TileRect::new(0, 5, 3, 5, 3),
                TileRect::new(0, 3, 5, 3, 5),
            ]
        );

        append(&mut rectangles, TileRect::new(0, 3, 3, 5, 5));
        assert_eq!(
            rectangles,
            vec![
                TileRect::new(0, 2, 2, 4, 4),
                TileRect::new(0, 1, 3, 1, 3),
                TileRect::new(0, 3, 1, 3, 1),
                TileRect::new(0, 5, 3, 5, 3),
                TileRect::new(0, 3, 5, 3, 5),
                TileRect::new(0, 5, 4, 5, 5),
                TileRect::new(0, 4, 5, 4, 5),
            ]
        );

        append(&mut rectangles, TileRect::new(0, 1, 1, 3, 3));
        assert_eq!(
            rectangles,
            vec![
                TileRect::new(0, 2, 2, 4, 4),
                TileRect::new(0, 1, 3, 1, 3),
                TileRect::new(0, 3, 1, 3, 1),
                TileRect::new(0, 5, 3, 5, 3),
                TileRect::new(0, 3, 5, 3, 5),
                TileRect::new(0, 5, 4, 5, 5),
                TileRect::new(0, 4, 5, 4, 5),
                TileRect::new(0, 1, 1, 1, 2),
                TileRect::new(0, 2, 1, 2, 1),
            ]
        );
    }
}
