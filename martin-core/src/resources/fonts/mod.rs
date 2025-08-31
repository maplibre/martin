//! Font processing and serving for map tile rendering.
//!
//! Provides font discovery, cataloging, and SDF (Signed Distance Field) glyph generation
//! in Protocol Buffer format for map rendering clients. Operates on 256-character Unicode
//! ranges (e.g., 0-255, 256-511) for efficient caching.
//!
//! # Usage
//!
//! ```rust,no_run
//! use martin_core::fonts::FontSources;
//! use martin_core::config::OptOneMany;
//! use std::path::PathBuf;
//!
//! let mut sources = FontSources::default();
//! sources.recursively_add_directory("/usr/share/fonts".into()).unwrap();
//! let font_data = sources.get_font_range("Arial,Helvetica", 0, 255).unwrap();
//! ```

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::LazyLock;

use bit_set::BitSet;
use dashmap::{DashMap, Entry};
use itertools::Itertools as _;
use log::{debug, info, warn};
use pbf_font_tools::freetype::{Face, Library};
use pbf_font_tools::prost::Message;
use pbf_font_tools::{Fontstack, Glyphs, render_sdf_glyph};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Maximum Unicode codepoint supported (U+FFFF - Basic Multilingual Plane).
const MAX_UNICODE_CP: usize = 0xFFFF;
/// Size of each Unicode codepoint range (256 characters).
const CP_RANGE_SIZE: usize = 256;
/// Font size in pixels for SDF glyph rendering.
const FONT_SIZE: usize = 24;
/// Font height in `FreeType`'s 26.6 fixed-point format.
#[allow(clippy::cast_possible_wrap)]
const CHAR_HEIGHT: isize = (FONT_SIZE as isize) << 6;
/// Buffer size in pixels around each glyph for SDF calculation.
const BUFFER_SIZE: usize = 3;
/// Radius in pixels for SDF distance calculation.
const RADIUS: usize = 8;
/// Cutoff threshold for SDF generation (0.0 to 1.0).
const CUTOFF: f64 = 0.25_f64;
/// Maximum Unicode codepoint range ID.
///
/// Each range is 256 codepoints long, so the highest range ID is 0xFFFF / 256 = 255.
const MAX_UNICODE_CP_RANGE_ID: usize = MAX_UNICODE_CP / CP_RANGE_SIZE;

mod error;
pub use error::FontError;

/// Glyph information: (codepoints, count, ranges, first, last).
type GetGlyphInfo = (BitSet, usize, Vec<(usize, usize)>, usize, usize);

/// Extracts available codepoints from a font face.
///
/// Returns `None` if the font contains no usable glyphs.
fn get_available_codepoints(face: &mut Face) -> Option<GetGlyphInfo> {
    let mut codepoints = BitSet::with_capacity(MAX_UNICODE_CP);
    let mut spans = Vec::new();
    let mut first: Option<usize> = None;
    let mut count = 0;

    for cp in 0..=MAX_UNICODE_CP {
        if face.get_char_index(cp).is_ok() {
            codepoints.insert(cp);
            count += 1;
            if first.is_none() {
                first = Some(cp);
            }
        } else if let Some(start) = first {
            spans.push((start, cp - 1));
            first = None;
        }
    }

    if count == 0 {
        None
    } else {
        let start = spans[0].0;
        let end = spans[spans.len() - 1].1;
        Some((codepoints, count, spans, start, end))
    }
}

/// Catalog mapping font names to metadata (e.g., "Arial" -> `CatalogFontEntry`).
pub type FontCatalog = HashMap<String, CatalogFontEntry>;

/// Font metadata including family, style, glyph count, and Unicode range.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogFontEntry {
    /// Font family name (e.g., "Arial").
    pub family: String,
    /// Font style (e.g., "Bold", "Italic").
    ///
    /// None for regular style.
    pub style: Option<String>,
    /// Total number of glyphs in this font.
    pub glyphs: usize,
    /// First Unicode codepoint available.
    pub start: usize,
    /// Last Unicode codepoint available.
    pub end: usize,
}

/// Thread-safe font manager for discovery, cataloging, and serving fonts as Protocol Buffers.
#[derive(Debug, Clone)]
pub struct FontSources {
    /// Map of font name to font source data.
    fonts: DashMap<String, FontSource>,
    /// Pre-computed bitmasks for each 256-character Unicode range.
    masks: Vec<BitSet>,
}

impl Default for FontSources {
    fn default() -> Self {
        let mut masks = Vec::with_capacity(MAX_UNICODE_CP_RANGE_ID + 1);

        let mut bs = BitSet::with_capacity(CP_RANGE_SIZE);
        for v in 0..=MAX_UNICODE_CP {
            bs.insert(v);
            if v % CP_RANGE_SIZE == (CP_RANGE_SIZE - 1) {
                masks.push(bs);
                bs = BitSet::with_capacity(CP_RANGE_SIZE);
            }
        }

        Self {
            fonts: DashMap::new(),
            masks,
        }
    }
}

impl FontSources {
    /// Discovers and loads fonts from the specified directory by recursively scanning for `.ttf`, `.otf`, and `.ttc` files.
    pub fn recursively_add_directory(&mut self, path: PathBuf) -> Result<(), FontError> {
        let lib = Library::init()?;
        recurse_dirs(&lib, path, &mut self.fonts, true)
    }

    /// Returns a catalog of all loaded fonts
    #[must_use]
    pub fn get_catalog(&self) -> FontCatalog {
        self.fonts
            .iter()
            .map(|v| (v.key().clone(), v.catalog_entry.clone()))
            .collect()
    }

    /// Generates Protocol Buffer encoded font data for a 256-character Unicode range.
    ///
    /// Combines multiple fonts (comma-separated) with later fonts filling gaps.
    /// Range must be exactly 256 characters (e.g., 0-255, 256-511).
    #[allow(clippy::cast_possible_truncation)]
    pub fn get_font_range(&self, ids: &str, start: u32, end: u32) -> Result<Vec<u8>, FontError> {
        if start > end {
            return Err(FontError::InvalidFontRangeStartEnd(start, end));
        }
        if start % (CP_RANGE_SIZE as u32) != 0 {
            return Err(FontError::InvalidFontRangeStart(start));
        }
        if end % (CP_RANGE_SIZE as u32) != (CP_RANGE_SIZE as u32 - 1) {
            return Err(FontError::InvalidFontRangeEnd(end));
        }
        if (end - start) != (CP_RANGE_SIZE as u32 - 1) {
            return Err(FontError::InvalidFontRange(start, end));
        }

        let mut needed = self.masks[(start as usize) / CP_RANGE_SIZE].clone();
        let fonts = ids
            .split(',')
            .filter_map(|id| match self.fonts.get(id) {
                None => Some(Err(FontError::FontNotFound(id.to_string()))),
                Some(v) => {
                    let mut ds = needed.clone();
                    ds.intersect_with(&v.codepoints);
                    if ds.is_empty() {
                        None
                    } else {
                        needed.difference_with(&v.codepoints);
                        Some(Ok((id, v, ds)))
                    }
                }
            })
            .collect::<Result<Vec<_>, FontError>>()?;

        if fonts.is_empty() {
            return Ok(Vec::new());
        }

        let lib = Library::init()?;
        let mut stack = Fontstack::default();

        for (id, font, ds) in fonts {
            if stack.name.is_empty() {
                stack.name = id.to_string();
            } else {
                let name = &mut stack.name;
                name.push_str(", ");
                name.push_str(id);
            }

            let face = lib.new_face(&font.path, font.face_index)?;

            // FreeType conventions: char width or height of zero means "use the same value"
            // and setting both resolution values to zero results in the default value
            // of 72 dpi.
            //
            // See https://www.freetype.org/freetype2/docs/reference/ft2-base_interface.html#ft_set_char_size
            // and https://www.freetype.org/freetype2/docs/tutorial/step1.html for details.
            face.set_char_size(0, CHAR_HEIGHT, 0, 0)?;

            for cp in &ds {
                let glyph = render_sdf_glyph(&face, cp as u32, BUFFER_SIZE, RADIUS, CUTOFF)?;
                stack.glyphs.push(glyph);
            }
        }

        stack.range = format!("{start}-{end}");

        let mut glyphs = Glyphs::default();
        glyphs.stacks.push(stack);
        Ok(glyphs.encode_to_vec())
    }
}

/// Internal font source data including path, face index, and available codepoints.
#[derive(Clone, Debug)]
pub struct FontSource {
    /// Path to the font file.
    path: PathBuf,
    /// Face index within the font file (for .ttc collections).
    face_index: isize,
    /// Unicode codepoints this font supports.
    codepoints: BitSet,
    /// Font metadata for the catalog.
    catalog_entry: CatalogFontEntry,
}

/// Recursively discovers fonts in directories and individual files.
/// Supports `.ttf`, `.otf`, and `.ttc` files.
fn recurse_dirs(
    lib: &Library,
    path: PathBuf,
    fonts: &mut DashMap<String, FontSource>,
    is_top_level: bool,
) -> Result<(), FontError> {
    let start_count = fonts.len();
    if path.is_dir() {
        for dir_entry in path
            .read_dir()
            .map_err(|e| FontError::IoError(e, path.clone()))?
            .flatten()
        {
            recurse_dirs(lib, dir_entry.path(), fonts, false)?;
        }
        if is_top_level && fonts.len() == start_count {
            return Err(FontError::NoFontFilesFound(path));
        }
    } else {
        if path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|e| ["otf", "ttf", "ttc"].contains(&e))
        {
            parse_font(lib, fonts, path.clone())?;
        }
        if is_top_level && fonts.len() == start_count {
            return Err(FontError::InvalidFontFilePath(path));
        }
    }

    Ok(())
}

/// Parses a font file and extracts all faces.
/// Font names are normalized (family + style, e.g., "Arial Bold").
fn parse_font(
    lib: &Library,
    fonts: &mut DashMap<String, FontSource>,
    path: PathBuf,
) -> Result<(), FontError> {
    static RE_SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\s|/|,)+").unwrap());

    let mut face = lib.new_face(&path, 0)?;
    let num_faces = face.num_faces() as isize;
    for face_index in 0..num_faces {
        if face_index > 0 {
            face = lib.new_face(&path, face_index)?;
        }
        let Some(family) = face.family_name() else {
            return Err(FontError::MissingFamilyName(path));
        };
        let mut name = family.clone();
        let style = face.style_name();
        if let Some(style) = &style {
            name.push(' ');
            name.push_str(style);
        }
        // Make sure font name has no slashes or commas, replacing them with spaces and de-duplicating spaces
        name = RE_SPACES.replace_all(name.as_str(), " ").to_string();

        match fonts.entry(name) {
            Entry::Occupied(v) => {
                warn!(
                    "Ignoring duplicate font {} from {} because it was already configured from {}",
                    v.key(),
                    path.display(),
                    v.get().path.display()
                );
            }
            Entry::Vacant(v) => {
                let key = v.key();
                let Some((codepoints, glyphs, ranges, start, end)) =
                    get_available_codepoints(&mut face)
                else {
                    warn!(
                        "Ignoring font {key} from {} because it has no available glyphs",
                        path.display()
                    );
                    continue;
                };

                info!(
                    "Configured font {key} with {glyphs} glyphs ({start:04X}-{end:04X}) from {}",
                    path.display()
                );
                debug!(
                    "Available font ranges: {}",
                    ranges
                        .iter()
                        .map(|(s, e)| if s == e {
                            format!("{s:02X}")
                        } else {
                            format!("{s:02X}-{e:02X}")
                        })
                        .join(", "),
                );

                v.insert(FontSource {
                    path: path.clone(),
                    face_index,
                    codepoints,
                    catalog_entry: CatalogFontEntry {
                        family,
                        style,
                        glyphs,
                        start,
                        end,
                    },
                });
            }
        }
    }

    Ok(())
}
