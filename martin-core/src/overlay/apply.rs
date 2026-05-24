//! [`OverlaySpec`] → side-effects on a maplibre [`Style`].
//!
//! Each [`OverlayFeature`] becomes one maplibre source plus its 1-2 layers.
//! The [`ID_PREFIX`] / synthetic-id scheme lives entirely here — callers
//! never see prefixed or unprefixed ids, only the typed features. Overlay
//! layers are added on top of the base style in feature order.

use maplibre_native::{
    CircleLayer, Color as MlnColor, FillLayer, GeoJson, GeoJsonError, GeoJsonSource, Layer,
    LineCap as MlnLineCap, LineJoin as MlnLineJoin, LineLayer, Static, Style, StyleError,
};

use crate::overlay::{
    CirclePaint, Color, FillPaint, ID_PREFIX, LineCap, LineJoin, LineLayout, LinePaint,
    OverlaySpec, StyledLayer,
};

/// Errors produced while applying an [`OverlaySpec`] to a [`Style`].
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    /// Failed to hand a feature's `GeoJSON` data to maplibre.
    #[error("overlay feature {index}: failed to convert GeoJSON: {source}")]
    GeoJsonConvert {
        /// Zero-based index of the offending feature.
        index: usize,
        /// Underlying maplibre error.
        #[source]
        source: GeoJsonError,
    },

    /// Maplibre rejected a source or layer mutation.
    #[error("overlay {id:?}: maplibre rejected style mutation: {source}")]
    Maplibre {
        /// Synthetic id that triggered the error (un-prefixed, e.g. `f0-fill`).
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
        layer_ids: Vec::new(),
        source_ids: Vec::with_capacity(spec.features.len()),
    };

    for (index, feature) in spec.features.iter().enumerate() {
        if let Err(err) = apply_feature(style, &mut applied, index, feature) {
            rollback(&mut applied, style);
            return Err(err);
        }
    }

    Ok(applied)
}

fn apply_feature(
    style: &mut Style<'_, Static>,
    applied: &mut AppliedOverlay,
    index: usize,
    feature: &crate::overlay::OverlayFeature,
) -> Result<(), ApplyError> {
    let source_id = format!("{ID_PREFIX}f{index}");
    let mln_gj: GeoJson = (&feature.data)
        .try_into()
        .map_err(|source| ApplyError::GeoJsonConvert { index, source })?;
    let mut gs = GeoJsonSource::new(&source_id);
    gs.set_geojson(&mln_gj);
    style
        .add_source(gs)
        .map_err(|source| ApplyError::Maplibre {
            id: format!("f{index}"),
            source,
        })?;
    applied.source_ids.push(source_id.clone());

    for styled in &feature.layers {
        let kind = match styled {
            StyledLayer::Fill(_) => "fill",
            StyledLayer::Line(..) => "line",
            StyledLayer::Circle(_) => "circle",
        };
        let layer_id = format!("{ID_PREFIX}f{index}-{kind}");
        let result = match styled {
            StyledLayer::Fill(paint) => add_fill(style, &layer_id, &source_id, paint),
            StyledLayer::Line(paint, layout) => {
                add_line(style, &layer_id, &source_id, paint, *layout)
            }
            StyledLayer::Circle(paint) => add_circle(style, &layer_id, &source_id, paint),
        };
        result.map_err(|source| ApplyError::Maplibre {
            id: format!("f{index}-{kind}"),
            source,
        })?;
        applied.layer_ids.push(layer_id);
    }

    Ok(())
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
    layer_id: &str,
    source_id: &str,
    paint: &FillPaint,
) -> Result<(), StyleError> {
    let mut layer = FillLayer::new(layer_id, source_id);
    if let Some(c) = paint.color {
        layer.set_fill_color(c.into());
    }
    if let Some(o) = paint.opacity {
        layer.set_fill_opacity(o);
    }
    if let Some(c) = paint.outline_color {
        layer.set_fill_outline_color(c.into());
    }
    style.add_layer(layer).map(|_| ())
}

fn add_line(
    style: &mut Style<'_, Static>,
    layer_id: &str,
    source_id: &str,
    paint: &LinePaint,
    layout: LineLayout,
) -> Result<(), StyleError> {
    let mut layer = LineLayer::new(layer_id, source_id);
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
    style.add_layer(layer).map(|_| ())
}

fn add_circle(
    style: &mut Style<'_, Static>,
    layer_id: &str,
    source_id: &str,
    paint: &CirclePaint,
) -> Result<(), StyleError> {
    let mut layer = CircleLayer::new(layer_id, source_id);
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
    style.add_layer(layer).map(|_| ())
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
