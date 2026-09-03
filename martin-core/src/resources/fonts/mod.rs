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

use std::collections::{BTreeMap, HashSet};
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

/// Maximum number of distinct font ids accepted in a single request.
const MAX_FONT_IDS_PER_REQUEST: usize = 128;

/// Deduplicates a comma-separated font id list, preserving order (font order
/// affects rendering priority, so unlike sprites this must not sort).
fn split_and_dedup_ids(ids: &str) -> Vec<&str> {
    let mut seen = HashSet::new();
    ids.split(',').filter(|id| seen.insert(*id)).collect()
}

/// Normalizes a font id list so equivalent fontstacks share one cache entry.
#[must_use]
pub fn normalize_font_ids(ids: &str) -> String {
    let mut unique_ids = split_and_dedup_ids(ids);
    unique_ids.sort_unstable();
    unique_ids.join(",")
}

mod error;
pub use error::FontError;

mod cache;
pub use cache::{FontCache, FontCacheKey, NO_FONT_CACHE, OptFontCache};

/// Glyph information: (codepoints, count, ranges, first, last).
type GetGlyphInfo = (BitSet, u32, Vec<(usize, usize)>, usize, usize);

/// Extracts available codepoints from a font face.
///
/// Returns `None` if the font contains no usable glyphs.
fn get_available_codepoints(face: &Face) -> Option<GetGlyphInfo> {
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
pub type FontCatalog = BTreeMap<String, CatalogFontEntry>;

/// Source font file container format.
///
/// The string serialization (serde and `strum`) is the lowercase file
/// extension, so [`FontFormat::VARIANTS`] doubles as the list of recognised
/// font extensions and [`str::parse`] maps an extension back to a variant.
#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    strum::EnumString,
    strum::VariantNames,
)]
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
    /// Map of alias name to the font names it combines, in fallback order.
    aliases: DashMap<String, Vec<String>>,
}

impl FontSources {
    /// Discovers and loads fonts from the specified directory by recursively scanning for `.ttf`, `.otf`, and `.ttc` files.
    pub fn recursively_add_directory(&mut self, path: PathBuf) -> Result<(), FontError> {
        let lib = Library::init()?;
        discover_fonts(&lib, path, &mut self.fonts)
    }

    /// Registers a named font stack that serves the given fonts combined, in fallback order.
    ///
    /// Every member must name an already discovered font, not another alias.
    /// An alias may share the name of a discovered font it references.
    /// Requests for such a name serve the alias.
    pub fn add_alias(&mut self, name: String, fonts: Vec<String>) -> Result<(), FontError> {
        if name.is_empty() || name.contains(',') {
            return Err(FontError::InvalidAliasName(name));
        }
        if fonts.is_empty() {
            return Err(FontError::EmptyAlias(name));
        }
        if fonts.len() > MAX_FONT_IDS_PER_REQUEST {
            return Err(FontError::TooManyFontsInAlias {
                alias: name,
                requested: fonts.len(),
                max: MAX_FONT_IDS_PER_REQUEST,
            });
        }
        for font in &fonts {
            if self.aliases.contains_key(font) {
                return Err(FontError::AliasWithinAlias {
                    alias: name,
                    font: font.clone(),
                });
            }
            if !self.fonts.contains_key(font) {
                return Err(FontError::AliasFontNotFound {
                    alias: name,
                    font: font.clone(),
                });
            }
        }
        if self.fonts.contains_key(&name) {
            info!(
                font.alias = %name,
                "Font alias shadows a font of the same name; requests for it will serve the alias"
            );
        }
        info!(
            font.alias = %name,
            font.names = %fonts.join(", "),
            "Configured font alias"
        );
        self.aliases.insert(name, fonts);
        Ok(())
    }

    /// Replaces every alias in a comma-separated font id list with its member
    /// fonts, preserving order and deduplicating.
    fn expanded_ids(&self, ids: &str) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut expanded = Vec::new();
        for id in split_and_dedup_ids(ids) {
            if let Some(alias) = self.aliases.get(id) {
                for font in alias.value() {
                    if seen.insert(font.clone()) {
                        expanded.push(font.clone());
                    }
                }
            } else if seen.insert(id.to_owned()) {
                expanded.push(id.to_owned());
            }
        }
        expanded
    }

    /// Expands every alias in a comma-separated font id list into its member fonts,
    /// preserving order and deduplicating.
    ///
    /// Callers that need the fonts a request actually resolves to use this,
    /// e.g. to build cache keys that can be invalidated per font.
    #[must_use]
    pub fn expand_font_ids(&self, ids: &str) -> String {
        if self.aliases.is_empty() {
            return ids.to_owned();
        }
        self.expanded_ids(ids).join(",")
    }

    /// Returns a catalog of all loaded fonts and aliases.
    ///
    /// An alias is listed under its own name.
    /// The `glyphs` value of an alias counts the distinct codepoints its member fonts cover.
    #[must_use]
    pub fn get_catalog(&self) -> FontCatalog {
        let mut catalog: FontCatalog = self
            .fonts
            .iter()
            .map(|v| (v.key().clone(), v.catalog_entry.clone()))
            .collect();
        for alias in &self.aliases {
            let mut codepoints = BitSet::new();
            let mut start = usize::MAX;
            let mut end = 0;
            for font in alias.value() {
                if let Some(font) = self.fonts.get(font) {
                    codepoints.union_with(&font.codepoints);
                    start = start.min(font.catalog_entry.start);
                    end = end.max(font.catalog_entry.end);
                }
            }
            let glyphs = u32::try_from(codepoints.count()).expect("codepoint count fits in u32");
            catalog.insert(
                alias.key().clone(),
                CatalogFontEntry {
                    family: alias.key().clone(),
                    style: None,
                    glyphs,
                    start,
                    end,
                    format: None,
                    last_modified_at: None,
                },
            );
        }
        catalog
    }

    /// Generates Protocol Buffer encoded font data for a 256-character Unicode range.
    ///
    /// Combines multiple fonts (comma-separated) with later fonts filling gaps.
    /// Ids may name aliases registered via [`Self::add_alias`], which expand to their member fonts.
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
            return Err(FontError::InvalidFontRangeStartEnd { start, end });
        }
        if start > end {
            return Err(FontError::InvalidFontRangeStartEnd { start, end });
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

        let unique_ids = self.expanded_ids(ids);
        if unique_ids.len() > MAX_FONT_IDS_PER_REQUEST {
            return Err(FontError::TooManyFontIds {
                requested: unique_ids.len(),
                max: MAX_FONT_IDS_PER_REQUEST,
            });
        }

        let fonts = unique_ids
            .iter()
            .map(|id| {
                if self.fonts.get(id.as_str()).is_none() {
                    return Err(FontError::FontNotFound(id.clone()));
                }

                Ok(id.as_str())
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
                id.clone_into(&mut stack.name);
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
                    get_available_codepoints(&face)
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
    use std::assert_matches;

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("tests")
            .join("fixtures")
            .join(name)
    }

    fn overpass_sources() -> FontSources {
        let mut sources = FontSources::default();
        sources.recursively_add_directory(fixture("fonts")).unwrap();
        sources
    }

    #[test]
    fn normalize_font_ids_collapses_order_and_duplicates() {
        assert_eq!(normalize_font_ids("b,a,b"), "a,b");
        assert_eq!(normalize_font_ids("a"), "a");
    }

    #[test]
    fn an_alias_serves_the_same_bytes_as_the_explicit_fontstack() {
        let mut sources = overpass_sources();
        sources
            .add_alias(
                "Overpass Mono".to_owned(),
                vec![
                    "Overpass Mono Regular".to_owned(),
                    "Overpass Mono Light".to_owned(),
                ],
            )
            .unwrap();

        assert_eq!(
            sources.expand_font_ids("Overpass Mono"),
            "Overpass Mono Regular,Overpass Mono Light"
        );
        assert_eq!(
            sources.expand_font_ids("Overpass Mono Light,Overpass Mono"),
            "Overpass Mono Light,Overpass Mono Regular",
            "a font shared between the request and the alias appears once"
        );
        let aliased = sources.get_font_range("Overpass Mono", 0, 255).unwrap();
        let explicit = sources
            .get_font_range("Overpass Mono Regular,Overpass Mono Light", 0, 255)
            .unwrap();
        assert_eq!(aliased, explicit);
    }

    #[test]
    fn an_alias_may_shadow_a_font_and_include_it() {
        let mut sources = overpass_sources();
        sources
            .add_alias(
                "Overpass Mono Regular".to_owned(),
                vec![
                    "Overpass Mono Regular".to_owned(),
                    "Overpass Mono Light".to_owned(),
                ],
            )
            .unwrap();

        let shadowed = sources
            .get_font_range("Overpass Mono Regular", 0, 255)
            .unwrap();
        // A fresh source set without the alias provides the explicit baseline.
        let explicit = overpass_sources()
            .get_font_range("Overpass Mono Regular,Overpass Mono Light", 0, 255)
            .unwrap();
        assert_eq!(shadowed, explicit);

        let catalog = sources.get_catalog();
        let entry = catalog
            .get("Overpass Mono Regular")
            .expect("the shadowed name stays cataloged");
        assert_eq!(entry.style, None, "the alias entry replaces the font's");
    }

    #[test]
    fn invalid_aliases_are_rejected() {
        let mut sources = overpass_sources();

        let err = sources
            .add_alias(
                "has,comma".to_owned(),
                vec!["Overpass Mono Regular".to_owned()],
            )
            .unwrap_err();
        assert_matches!(err, FontError::InvalidAliasName(_));

        let err = sources.add_alias("Empty".to_owned(), vec![]).unwrap_err();
        assert_matches!(err, FontError::EmptyAlias(_));

        let err = sources
            .add_alias("Unknown".to_owned(), vec!["Nonexistent".to_owned()])
            .unwrap_err();
        assert_matches!(err, FontError::AliasFontNotFound { .. });

        sources
            .add_alias("Stack".to_owned(), vec!["Overpass Mono Regular".to_owned()])
            .unwrap();
        let err = sources
            .add_alias("Nested".to_owned(), vec!["Stack".to_owned()])
            .unwrap_err();
        assert_matches!(err, FontError::AliasWithinAlias { .. });

        let too_many = vec!["Overpass Mono Regular".to_owned(); MAX_FONT_IDS_PER_REQUEST + 1];
        let err = sources.add_alias("Big".to_owned(), too_many).unwrap_err();
        assert_matches!(err, FontError::TooManyFontsInAlias { .. });
    }

    #[test]
    fn the_catalog_lists_aliases_with_merged_coverage() {
        let mut sources = overpass_sources();
        sources
            .recursively_add_directory(fixture("fonts2/u+3320.ttf"))
            .unwrap();
        sources
            .add_alias(
                "My Stack".to_owned(),
                vec![
                    "Overpass Mono Regular".to_owned(),
                    "DummyTestFont Regular".to_owned(),
                ],
            )
            .unwrap();

        let catalog = sources.get_catalog();
        let entry = catalog.get("My Stack").expect("alias is cataloged");
        insta::assert_json_snapshot!(entry, @r#"
        {
          "family": "My Stack",
          "glyphs": 935,
          "start": 0,
          "end": 128276
        }
        "#);
    }

    #[test]
    fn duplicate_ids_are_deduplicated_before_rendering() {
        let mut sources = FontSources::default();
        sources
            .recursively_add_directory(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("tests")
                    .join("fixtures")
                    .join("fonts")
                    .join("overpass-mono-regular.ttf"),
            )
            .unwrap();

        let single = sources
            .get_font_range("Overpass Mono Regular", 0, 255)
            .unwrap();
        let repeated_ids = vec!["Overpass Mono Regular"; 50].join(",");
        let repeated = sources.get_font_range(&repeated_ids, 0, 255).unwrap();

        assert_eq!(single, repeated);
    }

    #[test]
    fn too_many_ids_are_rejected_before_any_work() {
        let sources = FontSources::default();

        let ids = (0..=MAX_FONT_IDS_PER_REQUEST)
            .map(|i| format!("nonexistent{i}"))
            .collect::<Vec<_>>()
            .join(",");

        let Err(err) = sources.get_font_range(&ids, 0, 255) else {
            panic!("expected TooManyFontIds, got Ok");
        };
        assert_matches!(err, FontError::TooManyFontIds { .. });
    }

    #[test]
    fn exactly_max_ids_is_not_rejected_by_the_count_check() {
        let sources = FontSources::default();

        let ids = (0..MAX_FONT_IDS_PER_REQUEST)
            .map(|i| format!("nonexistent{i}"))
            .collect::<Vec<_>>()
            .join(",");

        let Err(err) = sources.get_font_range(&ids, 0, 255) else {
            panic!("expected FontNotFound, got Ok");
        };
        assert_matches!(err, FontError::FontNotFound(_));
    }

    #[cfg(unix)]
    #[test]
    fn k8s_configmap_symlinks_do_not_warn_about_duplicates() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let real_dir = root.join("..2024_05_17_17_57_51.390489675");
        std::fs::create_dir_all(&real_dir).unwrap();
        let font_src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("tests")
            .join("fixtures")
            .join("fonts2/u+3320.ttf");
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
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("tests")
            .join("fixtures")
            .join("fonts");
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
    fn available_codepoints() {
        let lib = Library::init().unwrap();

        // U+3320: SQUARE SANTIIMU, U+1F60A: SMILING FACE WITH SMILING EYES
        for codepoint in [0x3320, 0x1f60a] {
            let font_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../tests/fixtures/fonts2/u+{codepoint:x}.ttf"));
            assert!(font_path.is_file(), "{}", font_path.display());
            let face = lib.new_face(&font_path, 0).unwrap();

            let (_codepoints, count, _ranges, first, last) =
                get_available_codepoints(&face).unwrap();
            assert_eq!(count, 2);
            assert_eq!(format!("U+{first:X}"), format!("U+{codepoint:X}"));
            assert_eq!(format!("U+{last:X}"), format!("U+{codepoint:X}"));
        }
    }
}
