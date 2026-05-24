//! Tiny-skia-based drawing of overlays onto an `RgbaImage`.

use image::RgbaImage;
use thiserror::Error;
use tiny_skia::{FillRule, Paint, PathBuilder, PixmapMut, Stroke as SkStroke, Transform};

use crate::overlay::project::geo_to_pixel;
use crate::overlay::{Marker, OverlayView, Rgba, Shape};

/// Errors produced while drawing overlays.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum DrawError {
    /// The image dimensions were zero or too large for a `tiny_skia::PixmapMut`.
    #[error("cannot wrap pixmap for {width}x{height} image (zero, or width*height*4 overflows)")]
    InvalidImageSize {
        /// The requested pixmap width.
        width: u32,
        /// The requested pixmap height.
        height: u32,
    },
}

/// Draw overlays onto an existing image in place.
///
/// Returns `Ok(())` (and skips all work) when both slices are empty. Returns
/// [`DrawError::InvalidImageSize`] if the image's dimensions cannot back a
/// pixmap. On success, `img` is overwritten with the composited result.
///
/// The image's RGBA buffer is mutated in place: premultiplied for tiny-skia,
/// drawn into via [`PixmapMut`], then demultiplied back. No intermediate
/// `Pixmap` is allocated, so peak memory equals the input image size.
pub fn draw_overlays_into(
    img: &mut RgbaImage,
    shapes: &[Shape],
    markers: &[Marker],
    view: OverlayView,
) -> Result<(), DrawError> {
    if shapes.is_empty() && markers.is_empty() {
        return Ok(());
    }

    let (width, height) = (img.width(), img.height());
    let mut pixmap = PixmapMut::from_bytes(img.as_mut(), width, height)
        .ok_or(DrawError::InvalidImageSize { width, height })?;

    premultiply_in_place(pixmap.data_mut());

    let identity = Transform::identity();
    let mut rings: Vec<Vec<(f64, f64)>> = Vec::new();
    for shape in shapes {
        draw_shape(&mut pixmap, shape, view, identity, &mut rings);
    }
    for marker in markers {
        draw_marker(&mut pixmap, marker, view, identity);
    }

    demultiply_in_place(pixmap.data_mut());
    Ok(())
}

fn draw_shape(
    pixmap: &mut PixmapMut<'_>,
    shape: &Shape,
    view: OverlayView,
    identity: Transform,
    rings: &mut Vec<Vec<(f64, f64)>>,
) {
    rings.clear();
    let project =
        |c: &geo_types::Coord| geo_to_pixel(*c, view.zoom, view.width, view.height, view.center);

    match shape {
        Shape::Line { points, stroke } => {
            rings.push(points.iter().map(project).collect());
            if let Some(path) = build_subpaths(rings, false) {
                pixmap.stroke_path(
                    &path,
                    &to_paint(stroke.color),
                    &sk_stroke(stroke.width),
                    identity,
                    None,
                );
            }
        }
        Shape::Polygon {
            outer,
            holes,
            stroke,
            fill,
        } => {
            rings.push(outer.iter().map(project).collect());
            for hole in holes {
                rings.push(hole.iter().map(project).collect());
            }

            // EvenOdd fill across outer + holes turns nested closed subpaths into a polygon-with-holes.
            if let Some(fill) = fill
                && let Some(path) = build_subpaths(rings, true)
            {
                pixmap.fill_path(
                    &path,
                    &to_paint(fill.color),
                    FillRule::EvenOdd,
                    identity,
                    None,
                );
            }

            if let Some(stroke) = stroke
                && let Some(path) = build_subpaths(rings, false)
            {
                pixmap.stroke_path(
                    &path,
                    &to_paint(stroke.color),
                    &sk_stroke(stroke.width),
                    identity,
                    None,
                );
            }
        }
    }
}

fn draw_marker(
    pixmap: &mut PixmapMut<'_>,
    marker: &Marker,
    view: OverlayView,
    identity: Transform,
) {
    let (px, py) = geo_to_pixel(
        marker.coord,
        view.zoom,
        view.width,
        view.height,
        view.center,
    );

    #[expect(clippy::cast_possible_truncation, reason = "pixel coords fit in f32")]
    let (cx, cy) = (px as f32, py as f32);
    if let Some(circle) = PathBuilder::from_circle(cx, cy, marker.style.radius) {
        pixmap.fill_path(
            &circle,
            &to_paint(marker.style.color),
            FillRule::Winding,
            identity,
            None,
        );
    }
}

fn sk_stroke(width: f32) -> SkStroke {
    SkStroke {
        width,
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..SkStroke::default()
    }
}

fn to_paint(color: Rgba) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
    paint.anti_alias = true;
    // Force tiny-skia's f32 (HQ) pipeline in debug builds for stable test
    // output; release builds let tiny-skia pick the faster u16 pipeline. The
    // visual difference is imperceptible.
    paint.force_hq_pipeline = cfg!(debug_assertions);
    paint
}

/// Multiply a channel by alpha and divide by 255 with rounding.
/// Matches `tiny_skia::premultiply_u8` bit-for-bit.
#[inline]
fn mul_alpha(c: u8, a: u8) -> u8 {
    let prod = u32::from(c) * u32::from(a) + 128;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "result fits in u8 by construction"
    )]
    let out = ((prod + (prod >> 8)) >> 8) as u8;
    out
}

/// Premultiply RGBA bytes in place so tiny-skia can render into them.
fn premultiply_in_place(buf: &mut [u8]) {
    for chunk in buf.chunks_exact_mut(4) {
        let a = chunk[3];
        if a == 255 {
            continue;
        }
        chunk[0] = mul_alpha(chunk[0], a);
        chunk[1] = mul_alpha(chunk[1], a);
        chunk[2] = mul_alpha(chunk[2], a);
    }
}

/// Demultiply RGBA bytes in place after drawing.
/// Matches `tiny_skia::PremultipliedColorU8::demultiply` bit-for-bit.
fn demultiply_in_place(buf: &mut [u8]) {
    for chunk in buf.chunks_exact_mut(4) {
        let a = chunk[3];
        if a == 255 || a == 0 {
            // Opaque: bytes already match. Fully transparent: premul invariant
            // (r,g,b <= a) forces r=g=b=0, which is the demultiplied value too.
            continue;
        }
        let af = f64::from(a) / 255.0;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "result fits in u8: r/g/b <= a, so quotient <= 255"
        )]
        #[expect(clippy::cast_sign_loss, reason = "quotient is non-negative")]
        {
            chunk[0] = (f64::from(chunk[0]) / af + 0.5) as u8;
            chunk[1] = (f64::from(chunk[1]) / af + 0.5) as u8;
            chunk[2] = (f64::from(chunk[2]) / af + 0.5) as u8;
        }
    }
}

/// Build a tiny-skia path containing one subpath per ring, optionally closed for fills.
fn build_subpaths(rings: &[Vec<(f64, f64)>], closed: bool) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "pixel coords fit in f32 for image-sized canvases"
    )]
    for ring in rings {
        let Some(((first_x, first_y), rest)) = ring.split_first() else {
            continue;
        };
        pb.move_to(*first_x as f32, *first_y as f32);
        for &(x, y) in rest {
            pb.line_to(x as f32, y as f32);
        }
        if closed {
            pb.close();
        }
    }
    pb.finish()
}
