//! The hillshade bake from Mapzen *normal* tiles to L8 grayscale.
//!
//! The kernel
//! - reconstructs a surface normal from a normal tile's red/green channels,
//! - lights it,
//! - optionally quantises the result into hard toon bands,
//! - and lifts the shadow floor with an ambient term.
//!
//! It renders at `core_side` and 2x2-supersamples every output texel.
//! This bilinearly samples and shades each tap independently, then box-averages.
//! This means we bake area-coverage anti-aliasing into the band edges instead of leaving artefacts.

#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    reason = "decode/encode of the canvas index arithmetic is lossy and sign-changing casts unavoidable"
)]

use multiversion::multiversion;

use crate::tiles::neighbourhood::{CHANNELS, FIELD_SIDE as CANVAS, TILE_SIZE};

/// Tunable parameters of one hillshade bake.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BakeParams {
    /// Apron width in output texels, expressed at 256-core scale and rescaled
    /// with the core.
    ///
    /// An apron is an overdraw margin:
    /// the tile is rendered larger than its nominal size so a client doing anisotropic texture sampling has real
    /// data just past each edge instead of disagreeing with its neighbour about out-of-edge samples.
    /// Clients (e.g. `MapLibre`) that sample within tile bounds do not need one, so this defaults to `0`.
    pub padding: u32,
    /// Scales the normal's horizontal components before the normal is reconstructed, exaggerating relief.
    /// `1.0` is true-to-source.
    pub vertical_exaggeration: f64,
    /// Multiplier on the shade's deviation from neutral.
    /// Higher values push lit and shadowed slopes further apart.
    pub contrast: f64,
    /// How strongly high terrain deepens the contrast gain.
    pub elevation_scale: f64,
    /// Number of hard toon bands to quantise the shade into.
    ///
    /// Values below `2.0` disable banding entirely and produce a smooth gradient.
    pub toon_bands: f64,
    /// The darkest tone ("shadow floor") the bake will emit is lifted toward this.
    /// This makes shadows read as shaded rather than black.
    pub ambient: f64,
}

impl Default for BakeParams {
    fn default() -> Self {
        Self {
            padding: 0,
            vertical_exaggeration: 2.5,
            contrast: 2.5,
            elevation_scale: 0.0,
            toon_bands: 6.0,
            ambient: 0.2,
        }
    }
}

/// An L8 grayscale image and the framing that produced it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BakedTile {
    /// Side length of [`Self::gray`] in pixels, equal to `core_side + 2 * apron`.
    pub side: u32,
    /// Row-major L8 grayscale samples, `side * side` of them.
    pub gray: Vec<u8>,
    /// Apron width in output texels actually applied, after rescaling [`BakeParams::padding`] to the core.
    pub apron: u32,
}

/// A 3x3 tile neighbourhood as one `CANVAS` x `CANVAS` RGBA field.
///
/// Stored as raw `u8` and decoded to `f64` per tap to save on memory throughput.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canvas {
    /// `CANVAS * CANVAS * CHANNELS` bytes, row-major RGBA.
    rgba: Vec<u8>,
}

impl Canvas {
    /// Builds a canvas from a full row-major `CANVAS` x `CANVAS` RGBA buffer.
    #[must_use]
    pub(crate) fn from_rgba(rgba: Vec<u8>) -> Self {
        debug_assert_eq!(
            rgba.len(),
            CANVAS * CANVAS * CHANNELS,
            "canvas buffer must cover the whole 3x3 field"
        );
        Self { rgba }
    }

    /// A canvas whose every texel is `texel`.
    #[must_use]
    pub fn uniform(texel: [u8; CHANNELS]) -> Self {
        Self {
            rgba: texel
                .iter()
                .copied()
                .cycle()
                .take(CANVAS * CANVAS * CHANNELS)
                .collect(),
        }
    }

    /// The raw, undecoded texel at `(x, y)` in the assembled field.
    #[cfg(test)]
    pub(crate) fn raw_texel(&self, x: usize, y: usize) -> [u8; CHANNELS] {
        let base = (y * CANVAS + x) * CHANNELS;
        std::array::from_fn(|c| self.rgba[base + c])
    }

    /// True when every texel in the field is fully zero.
    #[cfg(test)]
    pub(crate) fn is_blank(&self) -> bool {
        self.rgba.iter().all(|&b| b == 0)
    }

    /// Decodes the texel at `(x, y)` to `[0, 1]` channels.
    #[inline]
    fn texel(&self, x: i64, y: i64) -> [f64; CHANNELS] {
        debug_assert!((0..(CANVAS as i64 - 1)).contains(&x), "texel not on canvas in x direction");
        debug_assert!((0..(CANVAS as i64 - 1)).contains(&y), "texel not on canvas in y direction");
        debug_assert!(self.rgba.len().is_multiple_of(CHANNELS));
        let (texels, _) = self.rgba.as_chunks::<CHANNELS>();

        texels[y as usize * CANVAS + x as usize].map(|c| f64::from(c) / 255.0)
    }

    /// Bilinear RGBA sample at canvas coordinates, where texel `(i, j)` has its centre at `(i + 0.5, j + 0.5)`.
    ///
    /// This reproduces what a GPU's linear filtering computes, which is the point:
    /// the supersample taps read the same field a client's raster shader would.
    #[inline]
    fn sample_bilinear(&self, x: f64, y: f64) -> [f64; CHANNELS] {
        let fx = x - 0.5;
        let fy = y - 0.5;
        let x0f = fx.floor();
        let y0f = fy.floor();
        let tx = fx - x0f;
        let ty = fy - y0f;
        let (x0, y0) = (x0f as i64, y0f as i64);
        let c00 = self.texel(x0, y0);
        let c01 = self.texel(x0 + 1, y0);
        let c10 = self.texel(x0, y0 + 1);
        let c11 = self.texel(x0 + 1, y0 + 1);
        std::array::from_fn(|c| {
            let top = c00[c] * (1.0 - tx) + c01[c] * tx;
            let bot = c10[c] * (1.0 - tx) + c11[c] * tx;
            top * (1.0 - ty) + bot * ty
        })
    }
}

/// The per-sample shade before banding and ambient, for all four taps of a texel's 2x2 supersample.
///
/// `nx`/`ny`/`alpha` are lanes of red/green/alpha in `[0, 1]`.
/// Blue is unused, since the vertical normal component is reconstructed rather than read..
#[multiversion(targets("x86_64+avx2", "x86_64+avx"))]
fn relief_shade(
    nx: [f64; CHANNELS],
    ny: [f64; CHANNELS],
    alpha: [f64; CHANNELS],
    light: [f64; 3],
    params: &BakeParams,
) -> [f64; CHANNELS] {
    let mut shade = [0.0; CHANNELS];
    for i in 0..CHANNELS {
        let nx = nx[i] * 2.0 - 1.0;
        let ny = ny[i] * 2.0 - 1.0;
        // Floored so normalisation never sees a null vector at low exaggeration.
        let nz = (1.0 - (nx * nx + ny * ny)).max(1e-4).sqrt();
        let vx = nx * params.vertical_exaggeration;
        let vy = ny * params.vertical_exaggeration;
        let len = (vx * vx + vy * vy + nz * nz).sqrt();
        // Normalise first, then dot. See the module's numeric contract.
        let s =
            ((vx / len) * light[0] + (vy / len) * light[1] + (nz / len) * light[2]).clamp(0.0, 1.0);
        // `1 - alpha` is the elevation proxy: it raises gain on high terrain.
        let elevation = 1.0 - alpha[i];
        let gain = params.contrast * (1.0 + params.elevation_scale * elevation);
        let neutral = light[2].clamp(0.0, 1.0);
        shade[i] = ((s - neutral) * gain + neutral).clamp(0.0, 1.0);
    }
    shade
}

/// Quantises each lane of `shade`'s deviation from `neutral` into `bands` hard steps.
///
/// Anchored at `neutral`, not zero, so flat ground lands on a band boundary instead of dithering.
#[multiversion(targets("x86_64+avx2", "x86_64+avx"))]
fn band_hard(shade: [f64; CHANNELS], neutral: f64, bands: f64) -> [f64; CHANNELS] {
    let band_size = 1.0 / bands;
    let mut out = [0.0; CHANNELS];
    for i in 0..CHANNELS {
        let t = (shade[i] - neutral) / band_size;
        // `t - floor(t)`, in [0, 1) for either sign of t. Not `f64::fract`.
        let floor_t = t.floor();
        let fract = t - floor_t;
        let step = if fract >= 0.5 { 1.0 } else { 0.0 };
        out[i] = (neutral + (floor_t + step) * band_size).clamp(0.0, 1.0);
    }
    out
}

/// Apron width in output texels for `core_side`, given a 256-scale `padding`.
///
/// Rescaling with the core keeps the apron covering the same fraction of ground
/// at every core size, so changing the core does not change the framing.
#[must_use]
pub fn output_apron(core_side: u32, padding: u32) -> u32 {
    (f64::from(padding) * f64::from(core_side) / TILE_SIZE as f64).round() as u32
}

/// Bakes one hillshade tile from `canvas`, lit by `light` used verbatim.
///
/// `core_side` is the nominal output size. The returned image is larger by the apron on every side.
#[must_use]
pub fn bake_with_light(
    canvas: &Canvas,
    core_side: u32,
    p: &BakeParams,
    light: [f64; 3],
) -> BakedTile {
    // Canvas texels per output texel
    // 1.0 is a 1:1 render of the centre tile.
    let scale = TILE_SIZE as f64 / f64::from(core_side);
    let apron = output_apron(core_side, p.padding);
    assert!(
        (f64::from(apron) + 1.0) * scale <= TILE_SIZE as f64,
        "padding {} is too large for core_side {core_side}: samples would fall outside the assembled 3x3 canvas",
        p.padding
    );
    let side = core_side + 2 * apron;
    let neutral = light[2].clamp(0.0, 1.0);
    let banded = p.toon_bands >= 2.0;

    // 2x2 regular-grid taps per output texel, in canvas units:
    // equivalent to rendering at twice the core and box-downsampling by two.
    let taps = [-0.25 * scale, 0.25 * scale];
    let taps_per_texel = (taps.len() * taps.len()) as f64;

    let mut gray = vec![0u8; (side as usize) * (side as usize)];
    for oy in 0..side {
        // Output texel centre in canvas coordinates.
        // The centre tile starts one tile in on both axes, hence the TILE_SIZE offset.
        let cy = (f64::from(oy) + 0.5 - f64::from(apron)) * scale + TILE_SIZE as f64;
        for ox in 0..side {
            let cx = (f64::from(ox) + 0.5 - f64::from(apron)) * scale + TILE_SIZE as f64;
            let mut nx = [0.0; CHANNELS];
            let mut ny = [0.0; CHANNELS];
            let mut alpha = [0.0; CHANNELS];
            let mut lane = 0;
            for dy in taps {
                for dx in taps {
                    let rgba = canvas.sample_bilinear(cx + dx, cy + dy);
                    nx[lane] = rgba[0];
                    ny[lane] = rgba[1];
                    alpha[lane] = rgba[3];
                    lane += 1;
                }
            }
            let mut shades = relief_shade(nx, ny, alpha, light, p);
            if banded {
                shades = band_hard(shades, neutral, p.toon_bands);
            }
            let acc = shades[0]
                .algebraic_add(shades[1])
                .algebraic_add(shades[2])
                .algebraic_add(shades[3]);
            // Ambient is affine, so lifting once after the average is identical to lifting every tap before it.
            let acc = acc.algebraic_div(taps_per_texel);
            let one_minus_ambient = 1.0_f64.algebraic_sub(p.ambient);
            let acc = one_minus_ambient.algebraic_mul(acc);
            let shade = p.ambient.algebraic_add(acc);
            let shade = shade.algebraic_mul(255.0).round() as u8;
            gray[(oy * side + ox) as usize] = shade;
        }
    }
    BakedTile { side, gray, apron }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const LIGHT_FROM_OVERHEAD: [f64; 3] = [0.0, 0.0, 1.0];
    const SLOPED_TEXEL: [u8; CHANNELS] = [160, 100, 200, 220];

    #[rstest]
    #[case(256, 8, 8)]
    #[case(512, 8, 16)]
    #[case(384, 8, 12)]
    #[case::rounds_away_from_zero(384, 1, 2)]
    #[case(512, 0, 0)]
    fn apron_rescales_with_the_core(
        #[case] core: u32,
        #[case] padding: u32,
        #[case] expected: u32,
    ) {
        assert_eq!(output_apron(core, padding), expected);
    }

    #[test]
    fn the_default_bake_is_exactly_the_core_size() {
        // no apron unless one is asked for, so a  512 core yields a plain 512x512 tile.
        let baked = bake_with_light(
            &Canvas::uniform(SLOPED_TEXEL),
            512,
            &BakeParams::default(),
            LIGHT_FROM_OVERHEAD,
        );
        assert_eq!(baked.apron, 0);
        assert_eq!(baked.side, 512);
        assert_eq!(baked.gray.len(), 512 * 512);
    }

    #[rstest]
    #[case(256, 8)]
    #[case(512, 8)]
    #[case(512, 0)]
    #[case(384, 1)]
    fn bake_dimensions_agree_with_the_apron_formula(#[case] core: u32, #[case] padding: u32) {
        let params = BakeParams {
            padding,
            ..BakeParams::default()
        };
        let baked =
            bake_with_light(&Canvas::uniform(SLOPED_TEXEL), core, &params, LIGHT_FROM_OVERHEAD);

        // Stated independently of the implementation.
        let expected_apron = (f64::from(padding) * f64::from(core) / 256.0).round() as u32;
        assert_eq!(baked.apron, expected_apron);
        assert_eq!(baked.side, core + 2 * expected_apron);
        assert_eq!(baked.gray.len(), (baked.side as usize).pow(2));
    }

    #[test]
    fn a_uniform_field_bakes_to_a_single_tone() {
        // with a uniform field, interpolation is the identity, so every tap of every
        // texel reads the same normal and the whole tile collapses to the per-texel shading.
        let baked = bake_with_light(
            &Canvas::uniform(SLOPED_TEXEL),
            64,
            &BakeParams {
                padding: 8,
                ..BakeParams::default()
            },
            [-0.5, 0.5, 0.7],
        );
        let first = baked.gray[0];
        assert!(
            baked.gray.iter().all(|&g| g == first),
            "a uniform normal field must bake to one flat tone"
        );
    }

    #[test]
    fn banding_switches_off_below_two_bands() {
        // Fewer than two bands cannot express a step, so the kernel serves the smooth gradient.
        let canvas = Canvas::uniform(SLOPED_TEXEL);
        let light = [-0.5, 0.5, 0.7];
        let smooth = |bands| {
            bake_with_light(
                &canvas,
                32,
                &BakeParams {
                    toon_bands: bands,
                    ..BakeParams::default()
                },
                light,
            )
            .gray[0]
        };

        // 3 bands must differ from off for this texel, which sits away from a band boundary.
        assert_eq!(smooth(0.0), smooth(1.9), "any value below 2 disables banding");
        assert_ne!(smooth(0.0), smooth(3.0), "banding must actually quantise");
    }

    fn varied_canvas() -> Canvas {
        let mut rgba = vec![0u8; CANVAS * CANVAS * CHANNELS];
        for y in 0..CANVAS {
            for x in 0..CANVAS {
                let base = (y * CANVAS + x) * CHANNELS;
                rgba[base] = ((x * 37 + y * 17) % 256) as u8;
                rgba[base + 1] = ((x * 11 + y * 53) % 256) as u8;
                rgba[base + 2] = 128;
                rgba[base + 3] = ((x * 3 + y * 29) % 256) as u8;
            }
        }
        Canvas::from_rgba(rgba)
    }

    #[test]
    fn bakes_a_varied_field_to_the_pinned_reference_bytes() {
        let canvas = varied_canvas();
        let light = [-0.5, 0.5, 0.7];
        let idx = [0usize, 17, 63, 64, 700, 2000, 4032, 4095];

        let baked = bake_with_light(&canvas, 64, &BakeParams::default(), light);
        let samples: Vec<u8> = idx.iter().map(|&i| baked.gray[i]).collect();
        assert_eq!(samples, vec![150, 150, 66, 100, 107, 109, 165, 107]);

        let smooth = BakeParams {
            toon_bands: 0.0,
            ..BakeParams::default()
        };
        let baked = bake_with_light(&canvas, 64, &smooth, light);
        let samples: Vec<u8> = idx.iter().map(|&i| baked.gray[i]).collect();
        assert_eq!(samples, vec![149, 147, 61, 93, 102, 108, 167, 102]);
    }

    #[test]
    fn ambient_sets_the_shadow_floor() {
        // The darkest tone a bake can emit is the ambient floor, so shadows read as shaded rather than as holes.
        let light = [0.0, 0.0, 1.0];
        // A steeply away-facing normal drives the raw shade to zero.
        let canvas = Canvas::uniform([255, 255, 0, 255]);
        for ambient in [0.0, 0.25, 0.5] {
            let baked = bake_with_light(
                &canvas,
                16,
                &BakeParams {
                    ambient,
                    ..BakeParams::default()
                },
                light,
            );
            let floor = (ambient * 255.0).round() as u8;
            assert_eq!(baked.gray[0], floor, "ambient {ambient} must floor the darkest tone");
        }
    }
}
