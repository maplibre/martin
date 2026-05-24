//! Simplestyle property reading and `tiny-skia` paint construction.
//!
//! Implements the subset of the [Mapbox simplestyle spec][spec] that the
//! overlay endpoint understands: `stroke`, `stroke-width`, `stroke-opacity`,
//! `fill`, `fill-opacity`, and `marker-color`.
//!
//! [spec]: https://github.com/mapbox/simplestyle-spec

use csscolorparser::Color;
use geojson::JsonObject;
use serde_json::Value as JsonValue;
use tiny_skia::Paint;

use crate::srv::static_overlay::parse::OverlayParseError;

/// Default stroke and fill color when properties don't set one (per simplestyle).
/// Polygons override this for `stroke`, defaulting to their `fill` color so a
/// fill-only polygon doesn't render with an unexpected outline.
pub(super) const DEFAULT_COLOR: &str = "#555555";
/// Default stroke width in pixels (per simplestyle).
pub(super) const DEFAULT_STROKE_WIDTH: f64 = 2.0;
/// Default fill opacity (per simplestyle).
pub(super) const DEFAULT_FILL_OPACITY: f64 = 0.6;

/// Resolve simplestyle stroke properties shared by lines and polygons.
/// `default_color` is the color used when the `stroke` property is absent -
/// lines pass [`DEFAULT_COLOR`], polygons pass their resolved `fill` color.
pub(super) fn stroke_paint(
    props: Option<&JsonObject>,
    default_color: &str,
) -> Result<(Option<Paint<'static>>, Option<f32>), OverlayParseError> {
    let stroke_color = str_prop(props, "stroke").unwrap_or(default_color);
    let stroke_opacity = f64_prop(props, "stroke-opacity").unwrap_or(1.0);
    let stroke = Some(paint_with_opacity("stroke", stroke_color, stroke_opacity)?);

    #[expect(
        clippy::cast_possible_truncation,
        reason = "stroke widths fit in f32 in practice; precision loss is acceptable"
    )]
    let width = Some(f64_prop(props, "stroke-width").unwrap_or(DEFAULT_STROKE_WIDTH) as f32);

    Ok((stroke, width))
}

pub(super) fn str_prop<'a>(props: Option<&'a JsonObject>, key: &str) -> Option<&'a str> {
    props?.get(key).and_then(JsonValue::as_str)
}

pub(super) fn f64_prop(props: Option<&JsonObject>, key: &str) -> Option<f64> {
    props?.get(key).and_then(JsonValue::as_f64)
}

/// Parse a CSS color and combine it with a `[0.0, 1.0]` opacity multiplier.
///
/// The opacity is multiplied with any alpha already encoded in the color
/// (e.g. `rgba(...)`) so simplestyle-spec's `stroke-opacity: 0.4` correctly
/// dims a fully opaque `stroke: "#312E81"`.
pub(super) fn paint_with_opacity(
    property: &'static str,
    color: &str,
    opacity: f64,
) -> Result<Paint<'static>, OverlayParseError> {
    let css: Color = color
        .trim()
        .parse()
        .map_err(|source| OverlayParseError::InvalidColor {
            property,
            value: color.to_string(),
            source,
        })?;
    let [r, g, b, base_alpha] = css.to_rgba8();
    let opacity = opacity.clamp(0.0, 1.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "opacity*255 fits in [0, 255]"
    )]
    let alpha = (f64::from(base_alpha) / 255.0 * opacity * 255.0).round() as u8;
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, alpha);
    paint.anti_alias = true;
    // Force tiny-skia's f32 (HQ) pipeline in debug builds for stable test
    // output; release builds let tiny-skia pick the faster u16 pipeline. The
    // visual difference is imperceptible.
    paint.force_hq_pipeline = cfg!(debug_assertions);
    Ok(paint)
}
