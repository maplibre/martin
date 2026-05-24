//! [`OverlaySpec`] → side-effects on a maplibre [`Style`].
//!
//! All caller ids are prepended with [`ID_PREFIX`] so overlays cannot collide
//! with base-style ids. The `before` reference in [`OverlayLayer`] is passed
//! to maplibre verbatim — it must reference a base-style layer.

use maplibre_native::{
    CircleLayer, Color as MlnColor, FillLayer, GeoJson, GeoJsonError, GeoJsonSource, Layer,
    LineCap as MlnLineCap, LineJoin as MlnLineJoin, LineLayer, Static, Style, StyleError,
};

use crate::overlay::{
    CirclePaint, Color, FillPaint, ID_PREFIX, LineCap, LineJoin, LineLayout, LinePaint,
    OverlayLayer, OverlaySpec,
};

/// Errors produced while applying an [`OverlaySpec`] to a [`Style`].
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    /// Failed to hand the source's `GeoJSON` data to maplibre.
    #[error("source {id:?}: failed to convert GeoJSON: {source}")]
    GeoJsonConvert {
        /// Source id that triggered the error (un-prefixed).
        id: String,
        /// Underlying maplibre error.
        #[source]
        source: GeoJsonError,
    },

    /// Maplibre rejected the source or layer mutation.
    #[error("{id:?}: maplibre rejected style mutation: {source}")]
    Maplibre {
        /// Id that triggered the error (un-prefixed).
        id: String,
        /// Underlying maplibre error.
        #[source]
        source: StyleError,
    },
}

/// Handle to overlay sources and layers added to a [`Style`]. Must be removed
/// from the style after the next render.
#[must_use = "AppliedOverlay must be removed from the style after rendering"]
#[derive(Debug)]
pub struct AppliedOverlay {
    layer_ids: Vec<String>,
    source_ids: Vec<String>,
}

impl AppliedOverlay {
    /// Remove every layer (in reverse-add order) then every source.
    /// Returns the style to a clean base.
    pub fn remove_from(self, style: &mut Style<'_, Static>) {
        for id in self.layer_ids.into_iter().rev() {
            style.remove_layer(&id);
        }
        for id in self.source_ids.into_iter().rev() {
            style.remove_source(&id);
        }
    }
}

/// Adds `spec` to `style`. On any failure mid-way, rolls back what this call
/// added (in reverse order) before returning `Err`.
///
/// # Errors
///
/// Returns [`ApplyError`] on `GeoJSON` conversion failure or any maplibre
/// rejection.
pub fn apply_to_style(
    spec: &OverlaySpec,
    style: &mut Style<'_, Static>,
) -> Result<AppliedOverlay, ApplyError> {
    let mut applied = AppliedOverlay {
        layer_ids: Vec::with_capacity(spec.layers.len()),
        source_ids: Vec::with_capacity(spec.sources.len()),
    };

    for src in &spec.sources {
        let prefixed = format!("{ID_PREFIX}{}", src.id);
        let mln_gj: GeoJson = match (&src.data).try_into() {
            Ok(gj) => gj,
            Err(source) => {
                rollback(&mut applied, style);
                return Err(ApplyError::GeoJsonConvert {
                    id: src.id.clone(),
                    source,
                });
            }
        };
        let mut gs = GeoJsonSource::new(&prefixed);
        gs.set_geojson(&mln_gj);
        if let Err(source) = style.add_source(gs) {
            rollback(&mut applied, style);
            return Err(ApplyError::Maplibre {
                id: src.id.clone(),
                source,
            });
        }
        applied.source_ids.push(prefixed);
    }

    for layer in &spec.layers {
        let result = match layer {
            OverlayLayer::Fill {
                id,
                source,
                before,
                paint,
            } => add_fill(style, id, source, before.as_deref(), paint),
            OverlayLayer::Line {
                id,
                source,
                before,
                paint,
                layout,
            } => add_line(style, id, source, before.as_deref(), paint, layout),
            OverlayLayer::Circle {
                id,
                source,
                before,
                paint,
            } => add_circle(style, id, source, before.as_deref(), paint),
        };
        match result {
            Ok(prefixed) => applied.layer_ids.push(prefixed),
            Err(err) => {
                rollback(&mut applied, style);
                return Err(err);
            }
        }
    }

    Ok(applied)
}

fn rollback(applied: &mut AppliedOverlay, style: &mut Style<'_, Static>) {
    for id in applied.layer_ids.drain(..).rev() {
        style.remove_layer(&id);
    }
    for id in applied.source_ids.drain(..).rev() {
        style.remove_source(&id);
    }
}

fn add_fill(
    style: &mut Style<'_, Static>,
    id: &str,
    source: &str,
    before: Option<&str>,
    paint: &FillPaint,
) -> Result<String, ApplyError> {
    let prefixed_id = format!("{ID_PREFIX}{id}");
    let prefixed_src = format!("{ID_PREFIX}{source}");
    let mut layer = FillLayer::new(&prefixed_id, &prefixed_src);
    if let Some(c) = paint.color {
        layer.set_fill_color(c.into());
    }
    if let Some(o) = paint.opacity {
        layer.set_fill_opacity(o);
    }
    if let Some(c) = paint.outline_color {
        layer.set_fill_outline_color(c.into());
    }
    push_layer(style, layer, id, before)?;
    Ok(prefixed_id)
}

fn add_line(
    style: &mut Style<'_, Static>,
    id: &str,
    source: &str,
    before: Option<&str>,
    paint: &LinePaint,
    layout: &LineLayout,
) -> Result<String, ApplyError> {
    let prefixed_id = format!("{ID_PREFIX}{id}");
    let prefixed_src = format!("{ID_PREFIX}{source}");
    let mut layer = LineLayer::new(&prefixed_id, &prefixed_src);
    if let Some(c) = paint.color {
        layer.set_line_color(c.into());
    }
    if let Some(o) = paint.opacity {
        layer.set_line_opacity(o);
    }
    if let Some(w) = paint.width {
        layer.set_line_width(w);
    }
    if let Some(cap) = layout.cap {
        layer.set_line_cap(cap.into());
    }
    if let Some(join) = layout.join {
        layer.set_line_join(join.into());
    }
    push_layer(style, layer, id, before)?;
    Ok(prefixed_id)
}

fn add_circle(
    style: &mut Style<'_, Static>,
    id: &str,
    source: &str,
    before: Option<&str>,
    paint: &CirclePaint,
) -> Result<String, ApplyError> {
    let prefixed_id = format!("{ID_PREFIX}{id}");
    let prefixed_src = format!("{ID_PREFIX}{source}");
    let mut layer = CircleLayer::new(&prefixed_id, &prefixed_src);
    if let Some(c) = paint.color {
        layer.set_circle_color(c.into());
    }
    if let Some(o) = paint.opacity {
        layer.set_circle_opacity(o);
    }
    if let Some(r) = paint.radius {
        layer.set_circle_radius(r);
    }
    if let Some(c) = paint.stroke_color {
        layer.set_circle_stroke_color(c.into());
    }
    if let Some(o) = paint.stroke_opacity {
        layer.set_circle_stroke_opacity(o);
    }
    if let Some(w) = paint.stroke_width {
        layer.set_circle_stroke_width(w);
    }
    push_layer(style, layer, id, before)?;
    Ok(prefixed_id)
}

fn push_layer<L: Layer>(
    style: &mut Style<'_, Static>,
    layer: L,
    id: &str,
    before: Option<&str>,
) -> Result<(), ApplyError> {
    let result = match before {
        Some(b) => style.add_layer_before(layer, b).map(|_| ()),
        None => style.add_layer(layer).map(|_| ()),
    };
    result.map_err(|source| ApplyError::Maplibre {
        id: id.to_string(),
        source,
    })
}

impl From<Color> for MlnColor {
    fn from(c: Color) -> Self {
        // csscolorparser already clamps to 0..=1, so this won't panic.
        Self::rgba(c.r, c.g, c.b, c.a)
    }
}

impl From<LineCap> for MlnLineCap {
    fn from(cap: LineCap) -> Self {
        match cap {
            LineCap::Butt => Self::Butt,
            LineCap::Round => Self::Round,
            LineCap::Square => Self::Square,
        }
    }
}

impl From<LineJoin> for MlnLineJoin {
    fn from(join: LineJoin) -> Self {
        match join {
            LineJoin::Miter => Self::Miter,
            LineJoin::Bevel => Self::Bevel,
            LineJoin::Round => Self::Round,
        }
    }
}
