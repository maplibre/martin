//! Indexed (palette) PNG encoding for rendered images.
//!
//! Map rasters are mostly flat fills, so a small per-image palette keeps them close to the RGBA
//! original at a fraction of the bytes. Each image gets the smallest palette that is good enough,
//! up to a configured maximum. Indices are packed at the smallest bit depth the palette allows and
//! written unfiltered, which is what compresses best here.

use std::collections::hash_map::Entry;
use std::num::TryFromIntError;

use fnv::FnvHashMap;
use image::RgbaImage;

/// Palette sizes tried in order: 16 is the largest 4-bit palette, then steps of about 1.5×.
const BUDGETS: [u16; 7] = [16, 24, 32, 48, 64, 96, 128];
/// Mean squared RGBA error per pixel, tuned on 1000 rendered basemap tiles.
const MAX_MSE: u64 = 2;
/// Squared RGBA error allowed for a colour covering a visible share of the image: about 5 levels
/// on each of three channels.
const MAX_VISIBLE_ERR: u32 = 75;
/// A colour on at least 1/`VISIBLE_SHARE` of the pixels is visible: 65 pixels of a 512×512 tile,
/// enough for a thin road or a label fill, while antialiasing pixels stay below it.
const VISIBLE_SHARE: u32 = 4000;

/// Errors raised while encoding an indexed PNG.
#[derive(thiserror::Error, Debug)]
pub enum PngPaletteError {
    #[error("A PNG palette holds 2 to 256 colours, not {0}")]
    MaxColorsOutOfRange(u16),

    #[error("Cannot encode a {width}x{height} image as a PNG")]
    EmptyImage { width: u32, height: u32 },

    #[error("Cannot encode a {width}x{height} image as an indexed PNG, it has too many pixels")]
    TooManyPixels { width: u32, height: u32 },

    #[error("Failed to quantise the image: {0}")]
    Quantize(#[from] quantizr::Error),

    #[error("Failed to write the indexed PNG: {0}")]
    Encoding(#[from] png::EncodingError),
}

/// Quantise `img` to at most `max_colors` colours, without dithering, and encode it as an
/// indexed PNG.
///
/// Tries each of `BUDGETS` below `max_colors`, then `max_colors` itself, and keeps the first
/// palette within `MAX_MSE` and `MAX_VISIBLE_ERR`. An image with no more distinct colours than
/// a budget is encoded losslessly.
pub fn encode(img: &RgbaImage, max_colors: u16) -> Result<Vec<u8>, PngPaletteError> {
    if !(2..=256).contains(&max_colors) {
        return Err(PngPaletteError::MaxColorsOutOfRange(max_colors));
    }
    let (width, height) = img.dimensions();
    if width == 0 || height == 0 {
        return Err(PngPaletteError::EmptyImage { width, height });
    }
    let too_many = |_: TryFromIntError| PngPaletteError::TooManyPixels { width, height };
    let pixels = u32::try_from(u64::from(width) * u64::from(height)).map_err(too_many)?;
    let w = usize::try_from(width).map_err(too_many)?;
    let h = usize::try_from(height).map_err(too_many)?;

    let qimg = quantizr::Image::new(img.as_raw(), w, h)?;
    let mut hist = quantizr::Histogram::new();
    hist.add_image(&qimg);
    let colors = Colors::of(img.as_raw(), pixels);
    let distinct = colors.counts.len();
    let uniq = quantizr::Image::new(&colors.uniq, distinct, 1)?;
    let mut lut = vec![0u8; distinct];

    let quantize = |budget: u16, lut: &mut [u8]| -> Result<_, PngPaletteError> {
        let mut opts = quantizr::Options::default();
        opts.set_max_colors(i32::from(budget))?;
        let mut res = quantizr::QuantizeResult::quantize_histogram(&hist, &opts);
        res.set_dithering_level(0.0)?;
        // An undithered remap is per colour, so remapping each distinct colour once is exact.
        res.remap_image(&uniq, lut)?;
        Ok(res)
    };
    let res = 'pick: {
        for budget in BUDGETS.into_iter().filter(|&b| b < max_colors) {
            let res = quantize(budget, &mut lut)?;
            if distinct <= usize::from(budget) || colors.good_enough(res.get_palette(), &lut) {
                break 'pick res;
            }
        }
        quantize(max_colors, &mut lut)?
    };
    let palette = res.get_palette();
    let palette: Vec<quantizr::Color> = palette
        .entries
        .iter()
        .zip(0..palette.count)
        .map(|(&color, _)| color)
        .collect();

    // Translucent entries first, so the tRNS chunk can stop at the last of them.
    let mut order: Vec<usize> = (0..palette.len()).collect();
    order.sort_by_key(|&i| palette[i].a == 255);
    let mut remap = [0u8; 256];
    for (new, &old) in (0..=u8::MAX).zip(&order) {
        remap[old] = new;
    }

    let (depth, bits) = match palette.len() {
        0..=2 => (png::BitDepth::One, 1),
        3..=4 => (png::BitDepth::Two, 2),
        5..=16 => (png::BitDepth::Four, 4),
        _ => (png::BitDepth::Eight, 8),
    };
    let per_byte = 8 / bits;
    let row_len = w.div_ceil(per_byte);
    let mut data = vec![0u8; row_len * h];
    for (y, row) in colors.ids.chunks_exact(w).enumerate() {
        for (x, &id) in row.iter().enumerate() {
            let shift = 8 - bits * (x % per_byte + 1);
            data[y * row_len + x / per_byte] |= remap[usize::from(lut[id])] << shift;
        }
    }

    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, width, height);
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(depth);
    enc.set_palette(
        order
            .iter()
            .flat_map(|&i| [palette[i].r, palette[i].g, palette[i].b])
            .collect::<Vec<u8>>(),
    );
    let trns: Vec<u8> = order
        .iter()
        .map(|&i| palette[i].a)
        .take_while(|&a| a != 255)
        .collect();
    if !trns.is_empty() {
        enc.set_trns(trns);
    }
    enc.set_deflate_compression(png::DeflateCompression::Level(6));
    enc.set_filter(png::Filter::NoFilter);
    let mut writer = enc.write_header()?;
    writer.write_image_data(&data)?;
    writer.finish()?;
    Ok(out)
}

/// The distinct colours of an image, how often each appears, and which one each pixel is.
struct Colors {
    uniq: Vec<u8>,
    counts: Vec<u32>,
    ids: Vec<usize>,
    pixels: u32,
}

impl Colors {
    fn of(rgba: &[u8], pixels: u32) -> Self {
        let mut slots = FnvHashMap::<u32, usize>::default();
        let (mut uniq, mut counts) = (Vec::new(), Vec::new());
        let mut ids = Vec::with_capacity(rgba.len() / 4);
        let (mut last, mut slot) = (None, 0);
        for px in rgba.as_chunks::<4>().0 {
            // Like quantizr, every alpha-0 pixel is transparent black.
            let key = if px[3] == 0 {
                0
            } else {
                u32::from_le_bytes(*px)
            };
            if last != Some(key) {
                last = Some(key);
                slot = match slots.entry(key) {
                    Entry::Occupied(entry) => *entry.get(),
                    Entry::Vacant(entry) => {
                        uniq.extend_from_slice(&key.to_le_bytes());
                        counts.push(0);
                        *entry.insert(counts.len() - 1)
                    }
                };
            }
            counts[slot] += 1;
            ids.push(slot);
        }
        Self {
            uniq,
            counts,
            ids,
            pixels,
        }
    }

    fn good_enough(&self, palette: &quantizr::Palette, lut: &[u8]) -> bool {
        let visible = self.pixels.div_ceil(VISIBLE_SHARE);
        let mut sq_err = 0u64;
        let uniq = self.uniq.as_chunks::<4>().0;
        for ((color, &count), &i) in uniq.iter().zip(&self.counts).zip(lut) {
            let entry = palette.entries[usize::from(i)];
            let d2 = sq_dist(*color, [entry.r, entry.g, entry.b, entry.a]);
            if count >= visible && d2 > MAX_VISIBLE_ERR {
                return false;
            }
            sq_err += u64::from(count) * u64::from(d2);
        }
        sq_err <= MAX_MSE * u64::from(self.pixels)
    }
}

fn sq_dist(a: [u8; 4], b: [u8; 4]) -> u32 {
    a.iter()
        .zip(b)
        .map(|(&a, b)| u32::from(a.abs_diff(b)).pow(2))
        .sum()
}

#[cfg(test)]
mod tests {
    use image::Rgba;
    use rstest::rstest;

    use super::*;

    struct Decoded {
        depth: png::BitDepth,
        palette: Vec<[u8; 4]>,
        rgba: RgbaImage,
    }

    fn decode(bytes: &[u8]) -> Decoded {
        let reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .expect("a PNG header");
        let info = reader.info();
        assert_eq!(info.color_type, png::ColorType::Indexed);
        let rgb = info.palette.as_deref().expect("a PLTE chunk");
        let trns = info.trns.as_deref().unwrap_or_default();
        let palette = rgb
            .as_chunks::<3>()
            .0
            .iter()
            .enumerate()
            .map(|(i, &[r, g, b])| [r, g, b, trns.get(i).copied().unwrap_or(255)])
            .collect();
        Decoded {
            depth: info.bit_depth,
            palette,
            rgba: image::load_from_memory(bytes).expect("decodes").to_rgba8(),
        }
    }

    fn encoded(img: &RgbaImage, max_colors: u16) -> Decoded {
        decode(&encode(img, max_colors).expect("encodes"))
    }

    fn distinct_colors(colors: u32) -> Vec<Rgba<u8>> {
        (0..colors)
            .map(|i| {
                let [r, g, b, _] = (i * 0x0097_3A1D).to_le_bytes();
                Rgba([r, g, b, 255])
            })
            .collect()
    }

    fn cycling(width: u32, height: u32, colors: &[Rgba<u8>]) -> RgbaImage {
        let mut cycle = colors.iter().copied().cycle();
        RgbaImage::from_fn(width, height, |_, _| {
            cycle.next().expect("a cycle never ends")
        })
    }

    fn flat_with_antialiasing() -> RgbaImage {
        let fills = [[230, 225, 215], [170, 210, 160], [255, 255, 255]];
        let mut img = RgbaImage::from_fn(256, 256, |x, _| {
            let [r, g, b] = fills[usize::from(x >= 86) + usize::from(x >= 172)];
            Rgba([r, g, b, 255])
        });
        for i in 0..240u8 {
            let px = img.get_pixel_mut(u32::from(i), u32::from(i / 2));
            px.0[0] = px.0[0].saturating_sub(i % 4 + 1);
            px.0[1] = px.0[1].saturating_sub(i % 3);
        }
        img
    }

    #[test]
    fn each_image_gets_the_smallest_palette_that_fits() {
        let images = [
            ("1 colour", cycling(37, 29, &distinct_colors(1))),
            ("2 colours", cycling(37, 29, &distinct_colors(2))),
            ("3 colours", cycling(37, 29, &distinct_colors(3))),
            ("16 colours", cycling(37, 29, &distinct_colors(16))),
            ("17 colours", cycling(37, 29, &distinct_colors(17))),
            ("256 colours", cycling(37, 29, &distinct_colors(256))),
            ("300 colours", cycling(64, 64, &distinct_colors(300))),
            ("flat fills with antialiasing", flat_with_antialiasing()),
        ];
        let table: Vec<String> = images
            .iter()
            .map(|(name, img)| {
                let decoded = encoded(img, 128);
                format!(
                    "{name}: {} colours at {:?}",
                    decoded.palette.len(),
                    decoded.depth
                )
            })
            .collect();
        insta::assert_snapshot!(table.join("\n"), @"
        1 colour: 1 colours at One
        2 colours: 2 colours at One
        3 colours: 3 colours at Two
        16 colours: 16 colours at Four
        17 colours: 17 colours at Eight
        256 colours: 128 colours at Eight
        300 colours: 128 colours at Eight
        flat fills with antialiasing: 16 colours at Four
        ");
    }

    #[rstest]
    #[case::one_colour(1)]
    #[case::two_colours(2)]
    #[case::three_colours(3)]
    #[case::sixteen_colours(16)]
    #[case::seventeen_colours(17)]
    #[case::all_256_colours(256)]
    fn an_image_within_the_palette_round_trips_losslessly(#[case] colors: u32) {
        let img = cycling(37, 29, &distinct_colors(colors));
        assert_eq!(encoded(&img, 256).rgba, img);
    }

    #[rstest]
    #[case::one_bit_one_column(2, 1)]
    #[case::one_bit_nine_columns(2, 9)]
    #[case::two_bits_three_columns(4, 3)]
    #[case::two_bits_five_columns(4, 5)]
    #[case::four_bits_seven_columns(16, 7)]
    #[case::four_bits_thirteen_columns(16, 13)]
    fn an_odd_width_packs_every_row(#[case] colors: u32, #[case] width: u32) {
        let img = cycling(width, 5, &distinct_colors(colors));
        assert_eq!(encoded(&img, 256).rgba, img);
    }

    #[test]
    fn more_colours_than_the_palette_map_to_their_nearest_entry() {
        let img = cycling(64, 64, &distinct_colors(300));
        let decoded = encoded(&img, 64);
        let farther_than_nearest: Vec<_> = img
            .pixels()
            .zip(decoded.rgba.pixels())
            .filter(|(px, out)| {
                let nearest = decoded.palette.iter().map(|&e| sq_dist(px.0, e)).min();
                Some(sq_dist(px.0, out.0)) != nearest
            })
            .collect();
        assert!(farther_than_nearest.is_empty(), "{farther_than_nearest:?}");
    }

    #[test]
    fn translucent_pixels_keep_their_alpha() {
        let img = cycling(
            9,
            4,
            &[
                Rgba([0, 0, 0, 0]),
                Rgba([200, 10, 10, 128]),
                Rgba([10, 200, 10, 255]),
            ],
        );
        assert_eq!(encoded(&img, 256).rgba, img);
    }

    #[test]
    fn fully_transparent_pixels_become_transparent_black() {
        let img = cycling(4, 4, &[Rgba([255, 0, 0, 0]), Rgba([0, 0, 255, 0])]);
        assert_eq!(encoded(&img, 256).palette, [[0, 0, 0, 0]]);
    }

    #[rstest]
    #[case::no_pixels(0, 0)]
    #[case::no_columns(0, 4)]
    #[case::no_rows(4, 0)]
    fn an_empty_image_is_rejected(#[case] width: u32, #[case] height: u32) {
        let err = encode(&RgbaImage::new(width, height), 256).expect_err("nothing to encode");
        assert!(
            matches!(err, PngPaletteError::EmptyImage { width: w, height: h } if (w, h) == (width, height)),
            "{err}"
        );
    }

    #[rstest]
    #[case::none(0)]
    #[case::one(1)]
    #[case::past_png(257)]
    fn a_palette_size_outside_png_is_rejected(#[case] colors: u16) {
        let img = cycling(4, 4, &distinct_colors(2));
        let err = encode(&img, colors).expect_err("no such palette");
        assert!(
            matches!(err, PngPaletteError::MaxColorsOutOfRange(c) if c == colors),
            "{err}"
        );
    }
}
