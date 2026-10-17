#![allow(dead_code, clippy::unwrap_used, clippy::cast_precision_loss)]

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use cog_tiles::{AsyncBackend, CogResult, MemoryBackend};

/// The z15 tile covered by the single-file fixtures (quadkey `031313131130112`).
pub const Z15_X: u32 = 16374;
pub const Z15_Y: u32 = 10897;

/// A test COG in the workspace's `tests/fixtures/cog/synthetic`.
pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures/cog/synthetic")
        .join(name)
}

pub fn memory(name: &str) -> MemoryBackend {
    let path = fixture(name);
    MemoryBackend::new(std::fs::read(&path).unwrap(), path.display().to_string())
}

/// The fixture pattern: band `b` of global pixel `(x, y)` is `(x + 2y + 64b + shift) mod 256`.
pub fn pattern(x: u32, y: u32, band: u32, shift: u32) -> u8 {
    u8::try_from((x + 2 * y + 64 * band + shift) % 256).unwrap()
}

/// The expected 256x256 tile of the pattern whose top-left pixel is global `(x0, y0)`.
pub fn expected_tile(x0: u32, y0: u32, bands: u32, shift: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(256 * 256 * bands as usize);
    for y in 0..256 {
        for x in 0..256 {
            for b in 0..bands {
                out.push(pattern(x0 + x, y0 + y, b, shift));
            }
        }
    }
    out
}

/// Decodes a PNG into `(width, height, channels, pixels)`.
pub fn decode_png(data: &[u8]) -> (u32, u32, usize, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(data.to_vec()))
        .read_info()
        .unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    (info.width, info.height, info.color_type.samples(), buf)
}

/// Decodes a JPEG into `(width, height, pixels)`.
pub fn decode_jpeg(data: &[u8]) -> (u16, u16, Vec<u8>) {
    let mut decoder = jpeg_decoder::Decoder::new(data);
    let pixels = decoder.decode().unwrap();
    let info = decoder.info().unwrap();
    (info.width, info.height, pixels)
}

pub fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let sum: u64 = a
        .iter()
        .zip(b)
        .map(|(&x, &y)| u64::from(x.abs_diff(y)))
        .sum();
    sum as f64 / a.len() as f64
}

/// Wraps a backend and counts the requests made to it.
#[derive(Debug)]
pub struct Counting<B> {
    pub inner: B,
    pub reads: Arc<AtomicUsize>,
}

impl<B> Counting<B> {
    pub fn new(inner: B) -> Self {
        Self {
            inner,
            reads: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl<B: AsyncBackend> AsyncBackend for Counting<B> {
    fn location(&self) -> &str {
        self.inner.location()
    }

    fn size(&self) -> u64 {
        self.inner.size()
    }

    async fn read(&self, range: Range<u64>) -> CogResult<Bytes> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.read(range).await
    }
}

/// Rewrites value `index` of `tag` in every IFD of a little-endian classic TIFF that has it,
/// for tests that need a slightly different file. Handles `SHORT` and `DOUBLE` tags.
pub fn patch_tag(bytes: &mut [u8], tag: u16, index: usize, change: impl Fn(f64) -> f64) {
    let u16_at = |b: &[u8], at: usize| u16::from_le_bytes(b[at..at + 2].try_into().unwrap());
    let u32_at = |b: &[u8], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    assert_eq!(&bytes[..4], b"II*\0", "not a little-endian classic TIFF");
    let mut ifd = u32_at(bytes, 4) as usize;
    while ifd != 0 {
        let count = usize::from(u16_at(bytes, ifd));
        for entry in (0..count).map(|i| ifd + 2 + i * 12) {
            if u16_at(bytes, entry) != tag {
                continue;
            }
            let kind = u16_at(bytes, entry + 2);
            assert!(
                matches!(kind, 3 | 12),
                "tag {tag} has unsupported type {kind}"
            );
            if kind == 3 {
                let at = entry + 8 + index * 2;
                let value = change(f64::from(u16_at(bytes, at)));
                #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                bytes[at..at + 2].copy_from_slice(&(value as u16).to_le_bytes());
            } else {
                let at = u32_at(bytes, entry + 8) as usize + index * 8;
                let value = change(f64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()));
                bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        ifd = u32_at(bytes, ifd + 2 + count * 12) as usize;
    }
}

/// A fixture read into memory with `edit` applied to its bytes.
pub fn patched(name: &str, edit: impl FnOnce(&mut Vec<u8>)) -> MemoryBackend {
    let path = fixture(name);
    let mut bytes = std::fs::read(&path).unwrap();
    edit(&mut bytes);
    MemoryBackend::new(bytes, path.display().to_string())
}
