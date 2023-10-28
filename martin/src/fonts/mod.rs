use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsStr;
use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::OnceLock;

use bit_set::BitSet;
use itertools::Itertools;
use log::{debug, info, warn};
use pbf_font_tools::freetype::{Face, Library};
use pbf_font_tools::protobuf::Message;
use pbf_font_tools::{render_sdf_glyph, Fontstack, Glyphs, PbfFontError};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::fonts::FontError::IoError;
use crate::OptOneMany;

const MAX_UNICODE_CP: usize = 0xFFFF;
const CP_RANGE_SIZE: usize = 256;
const FONT_SIZE: usize = 24;
#[allow(clippy::cast_possible_wrap)]
const CHAR_HEIGHT: isize = (FONT_SIZE as isize) << 6;
const BUFFER_SIZE: usize = 3;
const RADIUS: usize = 8;
const CUTOFF: f64 = 0.25_f64;

/// Each range is 256 codepoints long, so the highest range ID is 0xFFFF / 256 = 255.
const MAX_UNICODE_CP_RANGE_ID: usize = MAX_UNICODE_CP / CP_RANGE_SIZE;

#[derive(thiserror::Error, Debug)]
pub enum FontError {
    #[error("Font {0} not found")]
    FontNotFound(String),

    #[error("Font range start ({0}) must be <= end ({1})")]
    InvalidFontRangeStartEnd(u32, u32),

    #[error("Font range start ({0}) must be multiple of {CP_RANGE_SIZE} (e.g. 0, 256, 512, ...)")]
    InvalidFontRangeStart(u32),

    #[error(
        "Font range end ({0}) must be multiple of {CP_RANGE_SIZE} - 1 (e.g. 255, 511, 767, ...)"
    )]
    InvalidFontRangeEnd(u32),

    #[error("Given font range {0}-{1} is invalid. It must be {CP_RANGE_SIZE} characters long (e.g. 0-255, 256-511, ...)")]
    InvalidFontRange(u32, u32),

    #[error("FreeType font error: {0}")]
    FreeType(#[from] pbf_font_tools::freetype::Error),

    #[error("IO error accessing {}: {0}", .1.display())]
    IoError(std::io::Error, PathBuf),

    #[error("Invalid font file {}", .0.display())]
    InvalidFontFilePath(PathBuf),

    #[error("No font files found in {}", .0.display())]
    NoFontFilesFound(PathBuf),

    #[error("Font {} could not be loaded", .0.display())]
    UnableToReadFont(PathBuf),

    #[error("{0} in file {}", .1.display())]
    FontProcessingError(spreet::error::Error, PathBuf),

    #[error("Font {0} is missing a family name")]
    MissingFamilyName(PathBuf),

    #[error("PBF Font error: {0}")]
    PbfFontError(#[from] PbfFontError),

    #[error("Error serializing protobuf: {0}")]
    ErrorSerializingProtobuf(#[from] pbf_font_tools::protobuf::Error),
}

type GetGlyphInfo = (BitSet, usize, Vec<(usize, usize)>, usize, usize);

fn get_available_codepoints(face: &mut Face) -> Option<GetGlyphInfo> {
    let mut codepoints = BitSet::with_capacity(MAX_UNICODE_CP);
    let mut spans = Vec::new();
    let mut first: Option<usize> = None;
    let mut count = 0;

    for cp in 0..=MAX_UNICODE_CP {
        if face.get_char_index(cp) != 0 {
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

#[derive(Debug, Clone, Default)]
pub struct FontSources {
    fonts: HashMap<String, FontSource>,
    masks: Vec<BitSet>,
}

pub type FontCatalog = BTreeMap<String, CatalogFontEntry>;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogFontEntry {
    pub family: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    pub glyphs: usize,
    pub start: usize,
    pub end: usize,
}

impl FontSources {
    pub fn resolve(config: &mut OptOneMany<PathBuf>) -> Result<Self, FontError> {
        if config.is_empty() {
            return Ok(Self::default());
        }

        let mut fonts = HashMap::new();
        let lib = Library::init()?;

        for path in config.iter() {
            recurse_dirs(&lib, path.clone(), &mut fonts, true)?;
        }

        let mut masks = Vec::with_capacity(MAX_UNICODE_CP_RANGE_ID + 1);

        let mut bs = BitSet::with_capacity(CP_RANGE_SIZE);
        for v in 0..=MAX_UNICODE_CP {
            bs.insert(v);
            if v % CP_RANGE_SIZE == (CP_RANGE_SIZE - 1) {
                masks.push(bs);
                bs = BitSet::with_capacity(CP_RANGE_SIZE);
            }
        }

        Ok(Self { fonts, masks })
    }

    #[must_use]
    pub fn get_catalog(&self) -> FontCatalog {
        self.fonts
            .iter()
            .map(|(k, v)| (k.clone(), v.catalog_entry.clone()))
            .sorted_by(|(a, _), (b, _)| a.cmp(b))
            .collect()
    }

    /// Given a list of IDs in a format "id1,id2,id3", return a combined font.
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
        let mut stack = Fontstack::new();

        for (id, font, ds) in fonts {
            if stack.has_name() {
                let name = stack.mut_name();
                name.push_str(", ");
                name.push_str(id);
            } else {
                stack.set_name(id.to_string());
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

        stack.set_range(format!("{start}-{end}"));

        let mut glyphs = Glyphs::new();
        glyphs.stacks.push(stack);
        let mut result = Vec::new();
        glyphs.write_to_vec(&mut result)?;
        Ok(result)
    }
}

#[derive(Clone, Debug)]
pub struct FontSource {
    path: PathBuf,
    face_index: isize,
    codepoints: BitSet,
    catalog_entry: CatalogFontEntry,
}

fn recurse_dirs(
    lib: &Library,
    path: PathBuf,
    fonts: &mut HashMap<String, FontSource>,
    is_top_level: bool,
) -> Result<(), FontError> {
    let start_count = fonts.len();
    if path.is_dir() {
        for dir_entry in path
            .read_dir()
            .map_err(|e| IoError(e, path.clone()))?
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

fn parse_font(
    lib: &Library,
    fonts: &mut HashMap<String, FontSource>,
    path: PathBuf,
) -> Result<(), FontError> {
    static RE_SPACES: OnceLock<Regex> = OnceLock::new();

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
        name = RE_SPACES
            .get_or_init(|| Regex::new(r"(\s|/|,)+").unwrap())
            .replace_all(name.as_str(), " ")
            .to_string();

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
                        .collect::<Vec<_>>()
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
