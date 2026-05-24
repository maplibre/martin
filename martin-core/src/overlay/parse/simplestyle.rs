//! Simplestyle property reading and crate-owned color/style construction.
//!
//! Implements the subset of the [Mapbox simplestyle spec][spec] that the
//! overlay endpoint understands: `stroke`, `stroke-width`, `stroke-opacity`,
//! `fill`, `fill-opacity`, and `marker-color`.
//!
//! [spec]: https://github.com/mapbox/simplestyle-spec

use csscolorparser::Color;
use geojson::JsonObject;
use serde_json::Value as JsonValue;

use crate::overlay::Rgba;
use crate::overlay::parse::OverlayParseError;

/// Resolve simplestyle `stroke` + `stroke-width` for both lines and polygons.
///
/// `default_color` is used when the `stroke` property is absent: lines pass
/// the simplestyle default (`#555555`); polygons pass their resolved fill
/// color so that a fill-only polygon doesn't render with a contrasting
/// outline.
pub(super) fn resolve_stroke(
    props: Option<&JsonObject>,
    default_color: &str,
) -> Result<(Rgba, f32), OverlayParseError> {
    let stroke_color = str_prop(props, "stroke").unwrap_or(default_color);
    let stroke_opacity = f64_prop(props, "stroke-opacity")?.unwrap_or(1.0);
    let color = parse_color_with_opacity("stroke", stroke_color, stroke_opacity)?;

    #[expect(
        clippy::cast_possible_truncation,
        reason = "stroke widths fit in f32 in practice; precision loss is acceptable"
    )]
    let width = f64_prop(props, "stroke-width")?
        .unwrap_or(f64::from(crate::overlay::Stroke::DEFAULT_WIDTH)) as f32;

    Ok((color, width))
}

pub(super) fn str_prop<'a>(props: Option<&'a JsonObject>, key: &str) -> Option<&'a str> {
    props?.get(key).and_then(JsonValue::as_str)
}

/// Look up a numeric simplestyle property.
///
/// Returns `Ok(None)` when the key is absent or explicitly `null`, so callers
/// can fall back to a default with `.unwrap_or(...)`. Returns
/// [`OverlayParseError::NonNumericProperty`] when the value is present but is
/// not a JSON number — clients that stringify numerics (e.g. `"5"` for a
/// width) get a 400 instead of silently rendering with the default.
pub(super) fn f64_prop(
    props: Option<&JsonObject>,
    key: &'static str,
) -> Result<Option<f64>, OverlayParseError> {
    let Some(value) = props.and_then(|p| p.get(key)) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_f64()
        .map(Some)
        .ok_or_else(|| OverlayParseError::NonNumericProperty {
            property: key,
            value: value.clone(),
        })
}

/// Parse a CSS color and combine it with a `[0.0, 1.0]` opacity multiplier.
///
/// The opacity is multiplied with any alpha already encoded in the color
/// (e.g. `rgba(...)`) so simplestyle's `stroke-opacity: 0.4` correctly dims
/// a fully opaque `stroke: "#312E81"`.
pub(super) fn parse_color_with_opacity(
    property: &'static str,
    color: &str,
    opacity: f64,
) -> Result<Rgba, OverlayParseError> {
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
    let a = (f64::from(base_alpha) / 255.0 * opacity * 255.0).round() as u8;
    Ok(Rgba { r, g, b, a })
}

/// Default stroke/fill color when properties don't set one (per simplestyle).
/// Polygons override this for `stroke`, defaulting to their `fill` color so a
/// fill-only polygon doesn't render with an unexpected outline.
pub(super) const DEFAULT_COLOR_STR: &str = "#555555";

/// Default fill opacity (per simplestyle) as an `f64` for property defaulting.
pub(super) const DEFAULT_FILL_OPACITY: f64 = crate::overlay::Fill::DEFAULT_OPACITY as f64;
