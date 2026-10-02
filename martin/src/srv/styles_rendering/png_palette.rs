//! Indexed (palette) PNG encoding for rendered images.
//!
//! Map rasters are mostly flat fills, so a small per-image palette keeps them close to the RGBA
//! original at a fraction of the bytes. [`encode`] works in five steps:
//!
//! 1. Count the distinct colours of the image and how many pixels each one covers.
//! 2. Try palettes of growing size until the error is small: mean squared error per pixel, and
//!    the error of every colour large enough to be seen.
//! 3. Put translucent palette entries first, so the `tRNS` chunk stays short.
//! 4. Pack each pixel's palette index into 1, 2, 4 or 8 bits, the fewest the palette allows.
//! 5. Write the PNG without filtering, which compresses these images best.

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

type Rgba = [u8; 4];

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
    let columns = usize::try_from(width).map_err(too_many)?;
    let rows = usize::try_from(height).map_err(too_many)?;

    let colors = DistinctColors::of(img.as_raw(), pixels);
    let image = quantizr::Image::new(img.as_raw(), columns, rows)?;
    let (mut palette, mut palette_index_of_color) = pick_palette(&image, &colors, max_colors)?;
    translucent_first(&mut palette, &mut palette_index_of_color);
    let (depth, indices) = pack_indices(&colors, &palette_index_of_color, palette.len(), columns);
    write_png(width, height, depth, &palette, &indices)
}

/// The distinct colours of an image, how many pixels each covers, and which one each pixel is.
struct DistinctColors {
    colors: Vec<Rgba>,
    pixels_of_color: Vec<u32>,
    color_of_pixel: Vec<usize>,
    pixels: u32,
}

impl DistinctColors {
    fn of(rgba: &[u8], pixels: u32) -> Self {
        let mut index_of = FnvHashMap::<Rgba, usize>::default();
        let mut colors = Vec::new();
        let mut pixels_of_color = Vec::new();
        let mut color_of_pixel = Vec::with_capacity(rgba.len() / 4);
        let mut previous: Option<(Rgba, usize)> = None;
        for &px in rgba.as_chunks::<4>().0 {
            // Like quantizr, every alpha-0 pixel is transparent black.
            let color = if px[3] == 0 { [0; 4] } else { px };
            // Neighbouring pixels mostly share a colour, which skips the hash lookup.
            let index = match previous {
                Some((previous_color, index)) if previous_color == color => index,
                Some(_) | None => match index_of.entry(color) {
                    Entry::Occupied(entry) => *entry.get(),
                    Entry::Vacant(entry) => {
                        colors.push(color);
                        pixels_of_color.push(0);
                        *entry.insert(colors.len() - 1)
                    }
                },
            };
            previous = Some((color, index));
            pixels_of_color[index] += 1;
            color_of_pixel.push(index);
        }
        Self {
            colors,
            pixels_of_color,
            color_of_pixel,
            pixels,
        }
    }

    fn len(&self) -> usize {
        self.colors.len()
    }

    /// Whether `palette` is within `MAX_MSE` of the image and within `MAX_VISIBLE_ERR` of every
    /// visible colour.
    fn close_enough(&self, palette: &[Rgba], palette_index_of_color: &[u8]) -> bool {
        let visible = self.pixels.div_ceil(VISIBLE_SHARE);
        let mut sq_err = 0u64;
        for ((&color, &pixels), &index) in self
            .colors
            .iter()
            .zip(&self.pixels_of_color)
            .zip(palette_index_of_color)
        {
            let d2 = sq_dist(color, palette[usize::from(index)]);
            if pixels >= visible && d2 > MAX_VISIBLE_ERR {
                return false;
            }
            sq_err += u64::from(pixels) * u64::from(d2);
        }
        sq_err <= MAX_MSE * u64::from(self.pixels)
    }
}

/// The first palette of `BUDGETS` (below `max_colors`) that is close enough to the image, or one
/// of `max_colors` colours. An image with no more distinct colours than a budget gets them all.
///
/// Returns the palette and the palette index of each distinct colour. Without dithering a pixel's
/// index depends only on its colour, so remapping each distinct colour once covers every pixel.
fn pick_palette(
    image: &quantizr::Image,
    colors: &DistinctColors,
    max_colors: u16,
) -> Result<(Vec<Rgba>, Vec<u8>), PngPaletteError> {
    let mut histogram = quantizr::Histogram::new();
    histogram.add_image(image);
    let distinct = quantizr::Image::new(colors.colors.as_flattened(), colors.len(), 1)?;
    let mut palette_index_of_color = vec![0u8; colors.len()];
    for budget in BUDGETS.into_iter().filter(|&b| b < max_colors) {
        let palette = quantize(&histogram, &distinct, budget, &mut palette_index_of_color)?;
        if colors.len() <= usize::from(budget)
            || colors.close_enough(&palette, &palette_index_of_color)
        {
            return Ok((palette, palette_index_of_color));
        }
    }
    let palette = quantize(
        &histogram,
        &distinct,
        max_colors,
        &mut palette_index_of_color,
    )?;
    Ok((palette, palette_index_of_color))
}

/// A palette of at most `budget` colours for `histogram`, writing the palette index of each of
/// the `distinct` colours to `palette_index_of_color`.
fn quantize(
    histogram: &quantizr::Histogram,
    distinct: &quantizr::Image,
    budget: u16,
    palette_index_of_color: &mut [u8],
) -> Result<Vec<Rgba>, PngPaletteError> {
    let mut options = quantizr::Options::default();
    options.set_max_colors(i32::from(budget))?;
    let mut result = quantizr::QuantizeResult::quantize_histogram(histogram, &options);
    result.set_dithering_level(0.0)?;
    result.remap_image(distinct, palette_index_of_color)?;
    let palette = result.get_palette();
    let count = usize::try_from(palette.count).expect("a palette has at most 256 colours");
    Ok(palette
        .entries
        .iter()
        .take(count)
        .map(|c| [c.r, c.g, c.b, c.a])
        .collect())
}

/// Move translucent entries to the front of `palette`, keeping the order within each group, and
/// update `palette_index_of_color` to match.
fn translucent_first(palette: &mut Vec<Rgba>, palette_index_of_color: &mut [u8]) {
    let mut old_indices: Vec<usize> = (0..palette.len()).collect();
    old_indices.sort_by_key(|&old| palette[old][3] == 255);
    let mut new_index = [0u8; 256];
    for (new, &old) in (0..=u8::MAX).zip(&old_indices) {
        new_index[old] = new;
    }
    *palette = old_indices.iter().map(|&old| palette[old]).collect();
    for index in palette_index_of_color {
        *index = new_index[usize::from(*index)];
    }
}

/// Each pixel's palette index, packed row by row at the bit depth `palette_len` needs, most
/// significant bits first, each row starting on a new byte.
fn pack_indices(
    colors: &DistinctColors,
    palette_index_of_color: &[u8],
    palette_len: usize,
    columns: usize,
) -> (png::BitDepth, Vec<u8>) {
    let (depth, bits) = match palette_len {
        0..=2 => (png::BitDepth::One, 1),
        3..=4 => (png::BitDepth::Two, 2),
        5..=16 => (png::BitDepth::Four, 4),
        _ => (png::BitDepth::Eight, 8),
    };
    let per_byte = 8 / bits;
    let row_bytes = columns.div_ceil(per_byte);
    let rows = colors.color_of_pixel.len() / columns;
    let mut packed = vec![0u8; row_bytes * rows];
    for (row, packed_row) in colors
        .color_of_pixel
        .chunks_exact(columns)
        .zip(packed.chunks_exact_mut(row_bytes))
    {
        for (x, &color) in row.iter().enumerate() {
            let shift = 8 - bits * (x % per_byte + 1);
            packed_row[x / per_byte] |= palette_index_of_color[color] << shift;
        }
    }
    (depth, packed)
}

fn write_png(
    width: u32,
    height: u32,
    depth: png::BitDepth,
    palette: &[Rgba],
    indices: &[u8],
) -> Result<Vec<u8>, PngPaletteError> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Indexed);
    encoder.set_depth(depth);
    encoder.set_palette(
        palette
            .iter()
            .flat_map(|&[r, g, b, _]| [r, g, b])
            .collect::<Vec<u8>>(),
    );
    let alphas: Vec<u8> = palette
        .iter()
        .map(|&[.., a]| a)
        .take_while(|&a| a != 255)
        .collect();
    if !alphas.is_empty() {
        encoder.set_trns(alphas);
    }
    encoder.set_deflate_compression(png::DeflateCompression::Level(6));
    encoder.set_filter(png::Filter::NoFilter);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(indices)?;
    writer.finish()?;
    Ok(out)
}

fn sq_dist(a: Rgba, b: Rgba) -> u32 {
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

    fn flat_fills() -> RgbaImage {
        let fills = [[230, 225, 215], [170, 210, 160], [255, 255, 255]];
        RgbaImage::from_fn(256, 256, |x, _| {
            let [r, g, b] = fills[usize::from(x >= 86) + usize::from(x >= 172)];
            Rgba([r, g, b, 255])
        })
    }

    fn flat_with_antialiasing() -> RgbaImage {
        let mut img = flat_fills();
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

    fn flat_with_close_patches(patch_width: u32) -> RgbaImage {
        let mut img = flat_fills();
        for i in 0..20u8 {
            let color = Rgba([40 + (i % 8) * 20, 40 + (i / 8) * 60, 128, 255]);
            let (left, top) = (u32::from(i % 8) * 30, u32::from(i / 8) * 30);
            for x in left..left + patch_width {
                for y in top..top + 4 {
                    img.put_pixel(x, y, color);
                }
            }
        }
        img
    }

    #[rstest]
    #[case::patches_too_small_to_see(4, 16)]
    #[case::visible_patches(5, 23)]
    fn the_palette_grows_until_every_visible_colour_is_close(
        #[case] patch_width: u32,
        #[case] palette_len: usize,
    ) {
        let img = flat_with_close_patches(patch_width);
        assert_eq!(encoded(&img, 128).palette.len(), palette_len);
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
