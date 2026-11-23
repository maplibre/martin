//! File name templates that name the COG holding each tile of a mosaic.

use std::path::{Component, Path};

use crate::tile::{MAX_ZOOM, TileCoord};

/// Why a mosaic's file name template is invalid.
#[non_exhaustive]
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum MosaicTemplateError {
    /// A `}` without a `{` before it.
    #[error("unmatched `}}`")]
    UnmatchedClose,
    /// A `{` without a `}` after it.
    #[error("unmatched `{{`")]
    UnmatchedOpen,
    /// A placeholder other than the supported ones.
    #[error("unknown placeholder {{{0}}}, placeholders are {{quadkey}}, {{z}}, {{x}}, {{y}} and {{-y}}")]
    UnknownPlaceholder(String),
    /// Neither a quadkey nor both a column and a row.
    #[error("it must contain {{quadkey}}, or {{x}} and {{y}} (or {{-y}})")]
    MissingCoordinates,
    /// The names it renders leave the mosaic's location, e.g. through `..` or a leading `/`.
    #[error("file names must be relative paths inside the mosaic's location")]
    NotRelative,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Literal(String),
    Quadkey,
    Z,
    X,
    Y,
    /// The row counted from the bottom (TMS), `2^z - 1 - y`.
    FlippedY,
}

/// A pattern that names the file holding a tile, such as `{quadkey}.tif` or `{z}/{x}/{y}.tif`.
///
/// Placeholders are `{quadkey}`, `{z}`, `{x}`, `{y}` and `{-y}` (the row counted from the bottom,
/// as in TMS). In a mosaic they refer to the file's own tile at the file zoom, not to the requested tile.
///
/// ```
/// use cog_tiles::{KeyTemplate, TileCoord};
///
/// let template = KeyTemplate::new("ortho/{quadkey}.tif")?;
/// let tile = TileCoord::new(3, 2, 5)?;
/// assert_eq!(template.render(tile), "ortho/212.tif");
/// assert_eq!(template.parse("ortho/212.tif", 3), Some(tile));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyTemplate {
    template: String,
    parts: Vec<Part>,
}

impl KeyTemplate {
    /// Parses a template.
    ///
    /// # Errors
    /// Returns an error if the template has an unknown or unclosed placeholder, does not name both
    /// a column and a row, or renders names outside its location (e.g. through `..` or a leading `/`).
    pub fn new(template: &str) -> Result<Self, MosaicTemplateError> {
        let mut parts = Vec::new();
        let mut rest = template;
        while let Some(start) = rest.find(['{', '}']) {
            let (literal, tail) = rest.split_at(start);
            let Some(tail) = tail.strip_prefix('{') else {
                return Err(MosaicTemplateError::UnmatchedClose);
            };
            if !literal.is_empty() {
                parts.push(Part::Literal(literal.to_owned()));
            }
            let (name, after) = tail.split_once('}').ok_or(MosaicTemplateError::UnmatchedOpen)?;
            parts.push(match name {
                "quadkey" => Part::Quadkey,
                "z" => Part::Z,
                "x" => Part::X,
                "y" => Part::Y,
                "-y" => Part::FlippedY,
                _ => return Err(MosaicTemplateError::UnknownPlaceholder(name.to_owned())),
            });
            rest = after;
        }
        if !rest.is_empty() {
            parts.push(Part::Literal(rest.to_owned()));
        }
        let has = |part: &Part| parts.contains(part);
        if !(has(&Part::Quadkey) || has(&Part::X) && (has(&Part::Y) || has(&Part::FlippedY))) {
            return Err(MosaicTemplateError::MissingCoordinates);
        }
        let parsed = Self {
            template: template.to_owned(),
            parts,
        };
        // Placeholders render digits only, so one rendered name shows where all of them lead.
        let sample = parsed.render_parts(1, 0, 0);
        if !Path::new(&sample)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(MosaicTemplateError::NotRelative);
        }
        Ok(parsed)
    }

    /// The template as given.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.template
    }

    /// The name of the file whose own tile is `tile`.
    #[must_use]
    pub fn render(&self, tile: TileCoord) -> String {
        self.render_parts(tile.z(), tile.x(), tile.y())
    }

    /// The tile at zoom `z` of the file named `key`, if `key` is a name this template renders.
    #[must_use]
    pub fn parse(&self, key: &str, z: u8) -> Option<TileCoord> {
        let (mut x, mut y) = (None, None);
        let mut rest = key;
        for part in &self.parts {
            if let Part::Literal(text) = part {
                rest = rest.strip_prefix(text.as_str())?;
                continue;
            }
            let is_quadkey = matches!(part, Part::Quadkey);
            let end = rest
                .find(|c: char| !(c.is_ascii_digit() && (!is_quadkey || c <= '3')))
                .unwrap_or(rest.len());
            let (value, after) = rest.split_at(end);
            match part {
                Part::Quadkey => {
                    let (qx, qy) = from_quadkey(value, z)?;
                    (x, y) = (Some(qx), Some(qy));
                }
                Part::Z => (value.parse::<u8>().ok()? == z).then_some(())?,
                Part::X => x = Some(value.parse().ok()?),
                Part::Y => y = Some(value.parse().ok()?),
                Part::FlippedY => {
                    let flipped: u32 = value.parse().ok()?;
                    (u64::from(flipped) < 1_u64 << z).then_some(())?;
                    y = Some(flip_y(z, flipped));
                }
                Part::Literal(_) => {}
            }
            rest = after;
        }
        let tile = TileCoord::new(z, x?, y?).ok()?;
        // Reject names the template would spell differently, e.g. zero-padded numbers.
        (rest.is_empty() && self.render(tile) == key).then_some(tile)
    }

    fn render_parts(&self, z: u8, x: u32, y: u32) -> String {
        let mut key = String::with_capacity(self.template.len() + 16);
        for part in &self.parts {
            match part {
                Part::Literal(text) => key.push_str(text),
                Part::Quadkey => key.push_str(&quadkey(z, x, y)),
                Part::Z => key.push_str(&z.to_string()),
                Part::X => key.push_str(&x.to_string()),
                Part::Y => key.push_str(&y.to_string()),
                Part::FlippedY => key.push_str(&flip_y(z, y).to_string()),
            }
        }
        key
    }
}

/// Converts a row between XYZ (counted from the top) and TMS (counted from the bottom) at zoom `z`.
fn flip_y(z: u8, y: u32) -> u32 {
    let last = u32::try_from((1_u64 << z) - 1).unwrap_or(u32::MAX);
    last.saturating_sub(y)
}

/// The [Bing Maps quadkey](https://learn.microsoft.com/en-us/bingmaps/articles/bing-maps-tile-system)
/// of tile `z/x/y`: one base-4 digit per zoom level, `0` = NW, `1` = NE, `2` = SW, `3` = SE.
fn quadkey(z: u8, x: u32, y: u32) -> String {
    (1..=z)
        .rev()
        .map(|level| {
            let mask = 1 << (level - 1);
            let digit = u8::from(x & mask != 0) + 2 * u8::from(y & mask != 0);
            char::from(b'0' + digit)
        })
        .collect()
}

/// The column and row of a quadkey, if it has one digit per zoom level up to `z`.
fn from_quadkey(quadkey: &str, z: u8) -> Option<(u32, u32)> {
    if quadkey.len() != usize::from(z) || z > MAX_ZOOM {
        return None;
    }
    quadkey.bytes().try_fold((0_u32, 0_u32), |(x, y), digit| {
        let digit = match digit {
            b'0'..=b'3' => u32::from(digit - b'0'),
            _ => return None,
        };
        Some(((x << 1) | (digit & 1), (y << 1) | (digit >> 1)))
    })
}
