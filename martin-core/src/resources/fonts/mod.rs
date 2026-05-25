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
use std::sync::{Arc, LazyLock};

use bit_set::BitSet;
use chrono::{DateTime, Utc};
use dashmap::{DashMap, Entry};
use itertools::Itertools as _;
use pbf_font_tools::freetype::{Face, Library};
use pbf_font_tools::prost::Message as _;
use pbf_font_tools::{Fontstack, Glyphs, render_sdf_glyph};
use regex::Regex;
use serde::{Deserialize, Serialize};
use strum::VariantNames as _;
use tracing::{debug, info, instrument, warn};

use crate::walk_files;

/// Maximum Unicode codepoint supported.
///
/// Although U+FFFF covers the Basic Multilingual Plane, the Unicode standard
/// allows to use up to U+10FFFF, including for private use.
/// (cf. <https://en.wikipedia.org/wiki/Unicode_block>)
const MAX_UNICODE_CP: u32 = 0x0010_FFFF;
/// Size of each Unicode codepoint range (256 characters).
const CP_RANGE_SIZE: usize = 256;
/// Font size in pixels for SDF glyph rendering.
const FONT_SIZE: usize = 24;
/// Font height in `FreeType`'s 26.6 fixed-point format.
#[expect(clippy::cast_possible_wrap, reason = "FONT_SIZE << 6 is not wrapping")]
const CHAR_HEIGHT: isize = (FONT_SIZE as isize) << 6;
/// Buffer size in pixels around each glyph for SDF calculation.
const BUFFER_SIZE: usize = 3;
/// Radius in pixels for SDF distance calculation.
const RADIUS: usize = 8;
/// Cutoff threshold for SDF generation (0.0 to 1.0).
const CUTOFF: f64 = 0.25_f64;

mod error;
pub use error::FontError;

mod cache;
pub use cache::{FontCache, FontCacheKey, NO_FONT_CACHE, OptFontCache};

/// Glyph information: (codepoints, count, ranges, first, last).
type GetGlyphInfo = (BitSet, u32, Vec<(usize, usize)>, usize, usize);

/// Extracts available codepoints from a font face.
///
/// Returns `None` if the font contains no usable glyphs.
fn get_available_codepoints(face: &mut Face) -> Option<GetGlyphInfo> {
    let mut codepoints = BitSet::new();
    let mut spans = Vec::new();
    let mut first: Option<usize> = None;
    let mut last = 0;

    for (cp, _) in face.chars() {
        codepoints.insert(cp);
        if let Some(start) = first {
            if cp != last + 1 {
                spans.push((start, last));
                first = Some(cp);
            }
        } else {
            first = Some(cp);
        }
        last = cp;
    }

    if let Some(first) = first {
        spans.push((first, last));
        let count = u32::try_from(face.num_glyphs()).unwrap_or(0);
        let start = spans[0].0;
        Some((codepoints, count, spans, start, last))
    } else {
        None
    }
}

/// Catalog mapping font names to metadata (e.g., "Arial" -> `CatalogFontEntry`).
pub type FontCatalog = HashMap<String, CatalogFontEntry>;

/// Source font file container format.
///
/// The string serialization (serde and `strum`) is the lowercase file
/// extension, so [`FontFormat::VARIANTS`] doubles as the list of recognised
/// font extensions and [`str::parse`] maps an extension back to a variant.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[derive(strum::EnumString, strum::VariantNames)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
#[cfg_attr(
    feature = "unstable-schemas",
    derive(schemars::JsonSchema, utoipa::ToSchema)
)]
pub enum FontFormat {
    /// `OpenType` font (`.otf`)
    Otf,
    /// `TrueType` font (`.ttf`)
    Ttf,
    /// `TrueType` collection (`.ttc`)
    Ttc,
}

/// Font metadata including family, style, glyph count, and Unicode range.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "unstable-schemas",
    derive(schemars::JsonSchema, utoipa::ToSchema)
)]
pub struct CatalogFontEntry {
    /// Font family name (e.g., "Arial").
    pub family: String,
    /// Font style (e.g., "Bold", "Italic").
    ///
    /// None for regular style.
    pub style: Option<String>,
    /// Total number of glyphs in this font.
    pub glyphs: u32,
    /// First Unicode codepoint available.
    pub start: usize,
    /// Last Unicode codepoint available.
    pub end: usize,
    /// Source font file container format.
    pub format: Option<FontFormat>,
    /// Timestamp of the source font file's last modification.
    pub last_modified_at: Option<DateTime<Utc>>,
}

/// Thread-safe font manager for discovery, cataloging, and serving fonts as Protocol Buffers.
#[derive(Debug, Clone, Default)]
pub struct FontSources {
    /// Map of font name to font source data.
    fonts: DashMap<String, FontSource>,
}

impl FontSources {
    /// Discovers and loads fonts from the specified directory by recursively scanning for `.ttf`, `.otf`, and `.ttc` files.
    pub fn recursively_add_directory(&mut self, path: PathBuf) -> Result<(), FontError> {
        let lib = Library::init()?;
        discover_fonts(&lib, path, &mut self.fonts)
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
    #[expect(clippy::cast_possible_truncation)]
    #[instrument(
        level = "debug",
        skip(self),
        fields(
            font.fontstack = %ids,
            font.range.start = start,
            font.range.end = end,
        ),
        err(Debug),
    )]
    pub fn get_font_range(&self, ids: &str, start: u32, end: u32) -> Result<Vec<u8>, FontError> {
        if start > MAX_UNICODE_CP || end > MAX_UNICODE_CP {
            return Err(FontError::InvalidFontRangeStartEnd(start, end));
        }
        if start > end {
            return Err(FontError::InvalidFontRangeStartEnd(start, end));
        }
        if !start.is_multiple_of(CP_RANGE_SIZE as u32) {
            return Err(FontError::InvalidFontRangeStart(start));
        }
        if end % (CP_RANGE_SIZE as u32) != (CP_RANGE_SIZE as u32 - 1) {
            return Err(FontError::InvalidFontRangeEnd(end));
        }
        if (end - start) != (CP_RANGE_SIZE as u32 - 1) {
            return Err(FontError::InvalidFontRange(start, end));
        }

        let fonts = ids
            .split(',')
            .map(|id| {
                if self.fonts.get(id).is_none() {
                    return Err(FontError::FontNotFound(id.to_string()));
                }

                Ok(id)
            })
            .collect::<Result<Vec<&str>, FontError>>()?;

        if fonts.is_empty() {
            return Ok(Vec::new());
        }

        let lib = Library::init()?;
        let mut stack = Fontstack::default();

        for id in fonts {
            let Some(font) = self.fonts.get(id) else {
                continue;
            };

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

            for codepoint in start..=end {
                if !font.codepoints.contains(codepoint as usize) {
                    continue;
                }
                let g = render_sdf_glyph(&face, codepoint, BUFFER_SIZE, RADIUS, CUTOFF)?;
                stack.glyphs.push(g);
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
    codepoints: Arc<BitSet>,
    /// Font metadata for the catalog.
    catalog_entry: CatalogFontEntry,
}

/// Discovers fonts at `path` and registers them in `fonts`.
///
/// If `path` is
/// - a directory, we walked recursively, or
/// - if it is a single font file we register this
#[instrument(skip(lib, fonts), fields(path = ?path), err(Debug))]
fn discover_fonts(
    lib: &Library,
    path: PathBuf,
    fonts: &mut DashMap<String, FontSource>,
) -> Result<(), FontError> {
    if path.is_file() {
        if !path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|e| FontFormat::VARIANTS.contains(&e))
        {
            return Err(FontError::InvalidFontFilePath(path));
        }
        return parse_font(lib, fonts, path);
    }

    let start_count = fonts.len();
    let font_files = walk_files(&path, FontFormat::VARIANTS)
        .map_err(|e| FontError::IoError(e.into(), path.clone()))?;
    for font_path in font_files {
        parse_font(lib, fonts, font_path)?;
    }
    if fonts.len() == start_count {
        return Err(FontError::NoFontFilesFound(path));
    }
    Ok(())
}

/// Parses a font file and extracts all faces.
/// Font names are normalized (family + style, e.g., "Arial Bold").
#[instrument(skip(lib, fonts), fields(path = ?path), err(Debug))]
fn parse_font(
    lib: &Library,
    fonts: &mut DashMap<String, FontSource>,
    path: PathBuf,
) -> Result<(), FontError> {
    static RE_SPACES: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\s|/|,)+").expect("regex pattern is valid"));

    // The discovery filter only admits the lowercase extensions in
    // `FontFormat::VARIANTS`, so this parse succeeds for every file we reach.
    let format = path
        .extension()
        .and_then(OsStr::to_str)
        .and_then(|e| e.parse::<FontFormat>().ok());

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
                    font.name = %v.key(),
                    font.path.kept = %v.get().path.display(),
                    font.path.dropped = %path.display(),
                    "Ignoring duplicate font: already configured from another path"
                );
            }
            Entry::Vacant(v) => {
                let key = v.key();
                let Some((codepoints, glyphs, ranges, start, end)) =
                    get_available_codepoints(&mut face)
                else {
                    warn!(
                        font.name = %key,
                        font.path = %path.display(),
                        "Ignoring font: no available glyphs"
                    );
                    continue;
                };

                info!(
                    font.name = %key,
                    font.path = %path.display(),
                    font.glyph_count = glyphs,
                    font.range.start = start,
                    font.range.end = end,
                    "Configured font"
                );
                debug!(
                    font.name = %key,
                    font.ranges = %ranges
                        .iter()
                        .map(|(s, e)| if s == e {
                            format!("{s:02X}")
                        } else {
                            format!("{s:02X}-{e:02X}")
                        })
                        .join(", "),
                    "Available font ranges"
                );

                v.insert(FontSource {
                    path: path.clone(),
                    face_index,
                    codepoints: Arc::new(codepoints),
                    catalog_entry: CatalogFontEntry {
                        family,
                        style,
                        glyphs,
                        start,
                        end,
                        format,
                        // FIXME: stat the font file and surface its mtime.
                        last_modified_at: None,
                    },
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn k8s_configmap_symlinks_do_not_warn_about_duplicates() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let real_dir = root.join("..2024_05_17_17_57_51.390489675");
        std::fs::create_dir(&real_dir).unwrap();
        let font_src =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/fonts2/u+3320.ttf");
        std::fs::copy(&font_src, real_dir.join("u3320.ttf")).unwrap();
        symlink("..2024_05_17_17_57_51.390489675", root.join("..data")).unwrap();
        symlink("..data/u3320.ttf", root.join("u3320.ttf")).unwrap();

        let mut sources = FontSources::default();
        sources
            .recursively_add_directory(root.to_path_buf())
            .unwrap();
        assert_eq!(
            sources.get_catalog().len(),
            1,
            "expected exactly one font, not duplicates from the ..data/..timestamped tree"
        );
    }

    #[test]
    fn catalog_reports_font_format_from_extension() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/fonts");
        let mut sources = FontSources::default();
        sources.recursively_add_directory(dir).unwrap();

        let formats: Vec<FontFormat> = sources
            .get_catalog()
            .values()
            .filter_map(|e| e.format)
            .collect();
        assert!(
            formats.contains(&FontFormat::Ttf),
            "expected the .ttf fixture to report Ttf, got {formats:?}"
        );
        assert!(
            formats.contains(&FontFormat::Otf),
            "expected the .otf fixture to report Otf, got {formats:?}"
        );
    }

    #[test]
    fn test_get_available_codepoints() {
        let lib = Library::init().unwrap();

        // U+3320: SQUARE SANTIIMU, U+1F60A: SMILING FACE WITH SMILING EYES
        for codepoint in [0x3320, 0x1f60a] {
            let font_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../tests/fixtures/fonts2/u+{codepoint:x}.ttf"));
            assert!(font_path.is_file(), "{}", font_path.display());
            let mut face = lib.new_face(&font_path, 0).unwrap();

            let (_codepoints, count, _ranges, first, last) =
                get_available_codepoints(&mut face).unwrap();
            assert_eq!(count, 2);
            assert_eq!(format!("U+{first:X}"), format!("U+{codepoint:X}"));
            assert_eq!(format!("U+{last:X}"), format!("U+{codepoint:X}"));
        }
    }
}
