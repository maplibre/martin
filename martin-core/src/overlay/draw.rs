//! Tiny-skia-based drawing of overlays onto an `RgbaImage`.

use image::RgbaImage;
use thiserror::Error;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke as SkStroke, Transform};

use crate::overlay::project::geo_to_pixel;
use crate::overlay::{Marker, OverlayView, Rgba, Shape};

/// Errors produced while drawing overlays.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum DrawError {
    /// The image dimensions were zero or too large for a `tiny_skia::Pixmap`.
    #[error(
        "cannot allocate pixmap for {width}x{height} image (zero, or width*height*4 overflows)"
    )]
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
pub fn draw_overlays_into(
    img: &mut RgbaImage,
    shapes: &[Shape],
    markers: &[Marker],
    view: OverlayView,
) -> Result<(), DrawError> {
    if shapes.is_empty() && markers.is_empty() {
        return Ok(());
    }

    let mut pixmap = rgba_to_pixmap(img).ok_or(DrawError::InvalidImageSize {
        width: img.width(),
        height: img.height(),
    })?;
    let identity = Transform::identity();

    let mut rings: Vec<Vec<(f64, f64)>> = Vec::new();
    for shape in shapes {
        draw_shape(&mut pixmap, shape, view, identity, &mut rings);
    }
    for marker in markers {
        draw_marker(&mut pixmap, marker, view, identity);
    }

    pixmap_to_rgba_into(&pixmap, img);
    Ok(())
}

fn draw_shape(
    pixmap: &mut Pixmap,
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
                && rings[0].len() >= 3
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

fn draw_marker(pixmap: &mut Pixmap, marker: &Marker, view: OverlayView, identity: Transform) {
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

/// Convert an `RgbaImage` to a `tiny_skia::Pixmap` for drawing.
/// tiny-skia uses premultiplied alpha RGBA internally.
fn rgba_to_pixmap(img: &RgbaImage) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(img.width(), img.height())?;
    for (src, dst) in img
        .as_raw()
        .chunks_exact(4)
        .zip(pixmap.pixels_mut().iter_mut())
    {
        *dst = tiny_skia::ColorU8::from_rgba(src[0], src[1], src[2], src[3]).premultiply();
    }
    Some(pixmap)
}

fn pixmap_to_rgba_into(pixmap: &Pixmap, img: &mut RgbaImage) {
    for (src, dst) in pixmap.pixels().iter().zip(img.as_mut().chunks_exact_mut(4)) {
        let color = src.demultiply();
        dst[0] = color.red();
        dst[1] = color.green();
        dst[2] = color.blue();
        dst[3] = color.alpha();
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
