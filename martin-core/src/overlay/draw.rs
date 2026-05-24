//! Tiny-skia-based drawing of paths and markers onto an `RgbaImage`.

use image::RgbaImage;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::overlay::project::geo_to_pixel;
use crate::overlay::{MarkerOverlay, OverlayView, PathOverlay};

/// Default width when a `PathOverlay` doesn't carry one (matches simplestyle).
const DEFAULT_STROKE_WIDTH: f32 = 2.0;

/// Default fill of the classic Mapbox simplestyle marker; `marker-color` overrides it.
const DEFAULT_MARKER_FILL_RGBA: [u8; 4] = [255, 0, 0, 255];

/// Draw overlays onto a rendered image using tiny-skia for anti-aliased rendering.
///
/// Returns a new image; the caller's buffer is read once to build a Pixmap and
/// is never mutated, so taking `&RgbaImage` avoids a redundant clone at the
/// call site.
#[must_use]
pub fn draw_overlays(
    img: &RgbaImage,
    paths: &[PathOverlay],
    markers: &[MarkerOverlay],
    view: OverlayView,
) -> RgbaImage {
    let Some(mut pixmap) = rgba_to_pixmap(img) else {
        return img.clone();
    };
    let identity = Transform::identity();

    let mut rings: Vec<Vec<(f64, f64)>> = Vec::new();
    for path in paths {
        draw_path_overlay(&mut pixmap, path, view, identity, &mut rings);
    }
    for marker in markers {
        draw_marker_overlay(&mut pixmap, marker, view, identity);
    }

    pixmap_to_rgba(&pixmap)
}

fn draw_path_overlay(
    pixmap: &mut Pixmap,
    path: &PathOverlay,
    view: OverlayView,
    identity: Transform,
    rings: &mut Vec<Vec<(f64, f64)>>,
) {
    rings.clear();
    let project =
        |c: &geo_types::Coord| geo_to_pixel(*c, view.zoom, view.width, view.height, view.center);
    rings.push(path.points.iter().map(project).collect());
    for hole in &path.holes {
        rings.push(hole.iter().map(project).collect());
    }

    let stroke_width = path.width.unwrap_or(DEFAULT_STROKE_WIDTH);

    // EvenOdd fill across outer + holes turns nested closed subpaths into a polygon-with-holes.
    if let Some(fill) = path.fill.as_ref()
        && rings[0].len() >= 3
        && let Some(skia_path) = build_subpaths(rings, true)
    {
        pixmap.fill_path(&skia_path, fill, FillRule::EvenOdd, identity, None);
    }

    if let Some(stroke) = path.stroke.as_ref()
        && let Some(skia_path) = build_subpaths(rings, false)
    {
        let stroke_style = Stroke {
            width: stroke_width,
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Stroke::default()
        };
        pixmap.stroke_path(&skia_path, stroke, &stroke_style, identity, None);
    }
}

fn draw_marker_overlay(
    pixmap: &mut Pixmap,
    marker: &MarkerOverlay,
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
    let (cx, cy, radius) = (px as f32, py as f32, 8.0_f32);
    if let Some(circle) = PathBuilder::from_circle(cx, cy, radius) {
        let default_fill;
        let fill = if let Some(c) = marker.marker_color.as_ref() {
            c
        } else {
            default_fill = solid_paint(DEFAULT_MARKER_FILL_RGBA);
            &default_fill
        };
        pixmap.fill_path(&circle, fill, FillRule::Winding, identity, None);
    }
}

fn solid_paint([r, g, b, a]: [u8; 4]) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, a);
    paint.anti_alias = true;
    // Force tiny-skia's f32 (HQ) pipeline in debug builds for stable test
    // output; release builds let tiny-skia pick the faster u16 pipeline. The
    // visual difference is imperceptible.
    paint.force_hq_pipeline = cfg!(debug_assertions);
    paint
}

/// Convert an `RgbaImage` to a `tiny_skia::Pixmap` for drawing, then back.
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

fn pixmap_to_rgba(pixmap: &Pixmap) -> RgbaImage {
    let width = pixmap.width();
    let height = pixmap.height();
    let mut buf = vec![0u8; pixmap.data().len()];
    for (src, dst) in pixmap.pixels().iter().zip(buf.chunks_exact_mut(4)) {
        let color = src.demultiply();
        dst[0] = color.red();
        dst[1] = color.green();
        dst[2] = color.blue();
        dst[3] = color.alpha();
    }
    RgbaImage::from_raw(width, height, buf).expect("buffer length matches width*height*4")
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
