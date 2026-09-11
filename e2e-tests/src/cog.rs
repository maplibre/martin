//! Edits a checked-in COG fixture into one that violates a single requirement.

use std::fs;
use std::path::{Path, PathBuf};

use crate::fixture;

pub mod tag {
    pub const IMAGE_WIDTH: u16 = 256;
    pub const COMPRESSION: u16 = 259;
    pub const PHOTOMETRIC_INTERPRETATION: u16 = 262;
    pub const PLANAR_CONFIGURATION: u16 = 284;
    pub const TILE_WIDTH: u16 = 322;
    pub const MODEL_PIXEL_SCALE: u16 = 33550;
    pub const MODEL_TIEPOINT: u16 = 33922;
    pub const MODEL_TRANSFORMATION: u16 = 34264;
    pub const GEO_KEY_DIRECTORY: u16 = 34735;
}

pub const PROJECTED_CRS_GEO_KEY: u16 = 3072;

const TYPE_SHORT: u16 = 3;
const TYPE_DOUBLE: u16 = 12;
const ENTRY_LEN: usize = 20;
const INLINE_CAPACITY: usize = 8;
const BIG_TIFF_LE_HEADER: [u8; 4] = [b'I', b'I', 0x2B, 0x00];

/// A `tests/fixtures/cog` file loaded into memory, edited tag by tag and written back out under
/// a new name. Only the little-endian `BigTIFF` layout every fixture uses is handled.
#[derive(Debug, Clone)]
pub struct CogFixture {
    bytes: Vec<u8>,
}

impl CogFixture {
    #[must_use]
    pub fn new(name: &str) -> Self {
        let path = fixture(&format!("cog/{name}.tif"));
        let bytes = fs::read(&path)
            .unwrap_or_else(|e| panic!("failed to read the fixture {}: {e}", path.display()));
        assert_eq!(
            bytes[..4],
            BIG_TIFF_LE_HEADER,
            "{} is not a little-endian BigTIFF",
            path.display()
        );
        Self { bytes }
    }

    pub fn write_to(&self, dir: impl AsRef<Path>, name: &str) -> PathBuf {
        let path = dir.as_ref().join(format!("{name}.tif"));
        fs::write(&path, &self.bytes)
            .unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
        path
    }

    #[must_use]
    pub fn set_short(mut self, ifd: usize, tag: u16, value: u16) -> Self {
        let entry = self.entry(ifd, tag);
        let (kind, count) = self.entry_type(entry);
        assert_eq!(kind, TYPE_SHORT, "tag {tag} at ifd {ifd} is not a SHORT");
        assert_eq!(count, 1, "tag {tag} at ifd {ifd} holds more than one value");
        self.bytes[entry + 12..entry + 14].copy_from_slice(&value.to_le_bytes());
        self
    }

    #[must_use]
    pub fn set_double(mut self, ifd: usize, tag: u16, index: usize, value: f64) -> Self {
        let entry = self.entry(ifd, tag);
        let (kind, count) = self.entry_type(entry);
        assert_eq!(kind, TYPE_DOUBLE, "tag {tag} at ifd {ifd} is not a DOUBLE");
        assert!(
            index < count,
            "tag {tag} at ifd {ifd} has only {count} values"
        );
        let at = self.value_offset(entry) + index * 8;
        self.bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        self
    }

    /// Claim fewer values for a tag, leaving the values themselves in place. Growing the count is
    /// rejected, as it would point the tag past the bytes the file has.
    #[must_use]
    pub fn set_count(mut self, ifd: usize, tag: u16, count: usize) -> Self {
        let entry = self.entry(ifd, tag);
        let (_, declared) = self.entry_type(entry);
        assert!(
            count <= declared,
            "growing tag {tag} at ifd {ifd} from {declared} to {count} would point past its values"
        );
        self.bytes[entry + 4..entry + 12].copy_from_slice(&Self::eight_bytes(count));
        self
    }

    #[must_use]
    pub fn remove_tag(mut self, ifd: usize, tag: u16) -> Self {
        let entry = self.entry(ifd, tag);
        let ifd_at = self.ifd_offset(ifd);
        let count = self.entry_count(ifd_at);
        let after_entries = ifd_at + 8 + count * ENTRY_LEN;
        self.bytes
            .copy_within(entry + ENTRY_LEN..after_entries + 8, entry);
        self.bytes[ifd_at..ifd_at + 8].copy_from_slice(&Self::eight_bytes(count - 1));
        self
    }

    #[must_use]
    pub fn set_geo_key(mut self, key: u16, value: u16) -> Self {
        let at = self.geo_key_offset(key);
        self.bytes[at + 6..at + 8].copy_from_slice(&value.to_le_bytes());
        self
    }

    /// Overwrite the version, revision and key count the `GeoKeyDirectory` opens with.
    #[must_use]
    pub fn set_geo_key_header(mut self, header: [u16; 4]) -> Self {
        let at = self.value_offset(self.entry(0, tag::GEO_KEY_DIRECTORY));
        for (i, value) in header.iter().enumerate() {
            self.bytes[at + i * 2..at + i * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
        self
    }

    /// Respell the pixel scale and tiepoint as the `ModelTransformation` matrix they imply. Both
    /// describe the same north-up image, but they are read by different branches.
    #[must_use]
    pub fn into_model_transformation(mut self) -> Self {
        let scale = self.doubles(tag::MODEL_PIXEL_SCALE);
        let tiepoint = self.doubles(tag::MODEL_TIEPOINT);
        let matrix = [
            scale[0],
            0.0,
            0.0,
            tiepoint[3],
            0.0,
            -scale[1],
            0.0,
            tiepoint[4],
            0.0,
            0.0,
            0.0,
            tiepoint[5],
            0.0,
            0.0,
            0.0,
            1.0,
        ];

        let at = self.bytes.len();
        for value in matrix {
            self.bytes.extend_from_slice(&value.to_le_bytes());
        }
        // TIFF keeps IFD entries sorted by tag, so the matrix takes over the later of the two
        // entries it replaces and the earlier one is dropped.
        let entry = self.entry(0, tag::MODEL_TIEPOINT);
        self.bytes[entry..entry + 2].copy_from_slice(&tag::MODEL_TRANSFORMATION.to_le_bytes());
        self.bytes[entry + 2..entry + 4].copy_from_slice(&TYPE_DOUBLE.to_le_bytes());
        self.bytes[entry + 4..entry + 12].copy_from_slice(&Self::eight_bytes(matrix.len()));
        self.bytes[entry + 12..entry + 20].copy_from_slice(&Self::eight_bytes(at));
        self.remove_tag(0, tag::MODEL_PIXEL_SCALE)
    }

    fn doubles(&self, tag: u16) -> Vec<f64> {
        let entry = self.entry(0, tag);
        let (kind, count) = self.entry_type(entry);
        assert_eq!(kind, TYPE_DOUBLE, "tag {tag} is not a DOUBLE");
        let at = self.value_offset(entry);
        (0..count)
            .map(|i| f64::from_le_bytes(self.eight(at + i * 8)))
            .collect()
    }

    fn ifd_offset(&self, index: usize) -> usize {
        let mut at = self.offset(8);
        for _ in 0..index {
            let next = at + 8 + self.entry_count(at) * ENTRY_LEN;
            at = self.offset(next);
            assert_ne!(at, 0, "the fixture has fewer than {index} IFDs");
        }
        at
    }

    fn entry_count(&self, ifd_at: usize) -> usize {
        self.offset(ifd_at)
    }

    fn entry(&self, index: usize, tag: u16) -> usize {
        let ifd_at = self.ifd_offset(index);
        (0..self.entry_count(ifd_at))
            .map(|i| ifd_at + 8 + i * ENTRY_LEN)
            .find(|&entry| u16::from_le_bytes(self.two(entry)) == tag)
            .unwrap_or_else(|| panic!("ifd {index} of the fixture has no tag {tag}"))
    }

    fn entry_type(&self, entry: usize) -> (u16, usize) {
        (
            u16::from_le_bytes(self.two(entry + 2)),
            self.offset(entry + 4),
        )
    }

    fn value_offset(&self, entry: usize) -> usize {
        let (kind, count) = self.entry_type(entry);
        let width = match kind {
            TYPE_SHORT => 2,
            TYPE_DOUBLE => 8,
            other => panic!("tag type {other} is not one this helper reads"),
        };
        if count * width <= INLINE_CAPACITY {
            entry + 12
        } else {
            self.offset(entry + 12)
        }
    }

    fn offset(&self, at: usize) -> usize {
        usize::try_from(u64::from_le_bytes(self.eight(at)))
            .expect("the fixture is smaller than this platform can address")
    }

    fn eight_bytes(value: usize) -> [u8; 8] {
        u64::try_from(value)
            .expect("a usize always fits a u64")
            .to_le_bytes()
    }

    fn geo_key_offset(&self, key: u16) -> usize {
        let entry = self.entry(0, tag::GEO_KEY_DIRECTORY);
        let (_, count) = self.entry_type(entry);
        let at = self.value_offset(entry);
        (1..count / 4)
            .map(|i| at + i * 8)
            .find(|&tuple| u16::from_le_bytes(self.two(tuple)) == key)
            .unwrap_or_else(|| panic!("the fixture's GeoKeyDirectory has no key {key}"))
    }

    fn two(&self, at: usize) -> [u8; 2] {
        self.bytes[at..at + 2].try_into().expect("two bytes")
    }

    fn eight(&self, at: usize) -> [u8; 8] {
        self.bytes[at..at + 8].try_into().expect("eight bytes")
    }
}
