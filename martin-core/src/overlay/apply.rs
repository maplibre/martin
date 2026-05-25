//! [`OverlaySpec`] → side-effects on a maplibre [`Style`].
//!
//! Each [`OverlayFeature`] becomes one maplibre source plus the 1-2 layers its
//! geometry fans out to. The geometry→layer dispatch and the polygon
//! fill/outline rule live here -- they are rendering concerns, not validation.
//! A paint property left unset is simply not set on the layer, so it falls
//! through to `MapLibre`'s own default. The [`ID_PREFIX`] / synthetic-id scheme is
//! also local: callers only ever see the typed [`OverlayFeature`]s.

use geojson::{GeoJson as GjGeoJson, Geometry, GeometryValue};
use maplibre_native::{
    CircleLayer, Color as MlnColor, FillLayer, GeoJson, GeoJsonError, GeoJsonSource,
    LineCap as MlnLineCap, LineJoin as MlnLineJoin, LineLayer, Static, Style, StyleError,
};

use crate::overlay::{
    Color, ID_PREFIX, LineCap, LineJoin, OverlayFeature, OverlayProperties, OverlaySpec,
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
    let mut guard = OverlayGuard::new(style);
    for (index, feature) in spec.features.iter().enumerate() {
        guard.apply_feature(index, feature)?;
    }
    Ok(guard.commit())
}

/// Which layer kinds a feature fans out to, in draw order. A point draws a
/// circle, a line draws a line, and a polygon fills (unless only line
/// properties are set) and outlines (when any line property is present)
/// -- a bare polygon still fills so it stays visible. `None`/`GeometryCollection`
/// geometries produce nothing and are skipped.
fn layer_kinds(geometry: Option<&Geometry>, props: &OverlayProperties) -> Vec<LayerKind> {
    let Some(geometry) = geometry else {
        return Vec::new();
    };
    match geometry.value {
        GeometryValue::Point { .. } | GeometryValue::MultiPoint { .. } => vec![LayerKind::Circle],
        GeometryValue::LineString { .. } | GeometryValue::MultiLineString { .. } => {
            vec![LayerKind::Line]
        }
        GeometryValue::Polygon { .. } | GeometryValue::MultiPolygon { .. } => {
            let has_fill = props.fill_color.is_some()
                || props.fill_opacity.is_some()
                || props.fill_outline_color.is_some();
            let has_line = props.line_color.is_some()
                || props.line_opacity.is_some()
                || props.line_width.is_some()
                || props.line_cap.is_some()
                || props.line_join.is_some();
            let mut kinds = Vec::with_capacity(2);
            if has_fill || !has_line {
                kinds.push(LayerKind::Fill);
            }
            if has_line {
                kinds.push(LayerKind::Line);
            }
            kinds
        }
        GeometryValue::GeometryCollection { .. } => Vec::new(),
    }
}

/// `Display` renders the lowercase variant name, used as the layer-id suffix.
#[derive(Copy, Clone, strum::Display)]
#[strum(serialize_all = "lowercase")]
enum LayerKind {
    Fill,
    Line,
    Circle,
}

/// Scope guard for the apply pass: accumulates the ids it adds to the style
/// and, unless [`commit`](OverlayGuard::commit)ted, removes them again on
/// drop -- including on an early `?` return or a panic. This makes it
/// structurally impossible for a fallible step to leave a half-applied
/// overlay behind. The success path hands the ids off to an
/// [`AppliedOverlay`] for deferred removal after the render.
struct OverlayGuard<'a, 'st> {
    style: &'st mut Style<'a, Static>,
    layer_ids: Vec<String>,
    source_ids: Vec<String>,
    committed: bool,
}

impl<'a, 'st> OverlayGuard<'a, 'st> {
    fn new(style: &'st mut Style<'a, Static>) -> Self {
        Self {
            style,
            layer_ids: Vec::new(),
            source_ids: Vec::new(),
            committed: false,
        }
    }

    fn apply_feature(&mut self, index: usize, feature: &OverlayFeature) -> Result<(), ApplyError> {
        let props = feature.properties.clone().unwrap_or_default();
        let kinds = layer_kinds(feature.geometry.as_ref(), &props);
        if kinds.is_empty() {
            // null / unsupported geometry, or a geometry that draws nothing.
            return Ok(());
        }
        let geometry = feature
            .geometry
            .clone()
            .expect("layer_kinds is empty for a missing geometry");

        let source_id = format!("{ID_PREFIX}f{index}");
        let mln_gj: GeoJson = (&GjGeoJson::Geometry(geometry))
            .try_into()
            .map_err(|source| ApplyError::GeoJsonConvert { index, source })?;
        let mut gs = GeoJsonSource::new(&source_id);
        gs.set_geojson(&mln_gj);
        self.style
            .add_source(gs)
            .map_err(|source| ApplyError::Maplibre {
                id: format!("f{index}"),
                source,
            })?;
        self.source_ids.push(source_id.clone());

        for kind in kinds {
            let layer_id = format!("{ID_PREFIX}f{index}-{kind}");
            let result = match kind {
                LayerKind::Fill => add_fill(self.style, &layer_id, &source_id, &props),
                LayerKind::Line => add_line(self.style, &layer_id, &source_id, &props),
                LayerKind::Circle => add_circle(self.style, &layer_id, &source_id, &props),
            };
            result.map_err(|source| ApplyError::Maplibre {
                id: format!("f{index}-{kind}"),
                source,
            })?;
            self.layer_ids.push(layer_id);
        }

        Ok(())
    }

    /// Disarm the rollback and hand the accumulated ids to an
    /// [`AppliedOverlay`] for removal after the render.
    fn commit(mut self) -> AppliedOverlay {
        self.committed = true;
        AppliedOverlay {
            layer_ids: std::mem::take(&mut self.layer_ids),
            source_ids: std::mem::take(&mut self.source_ids),
        }
    }
}

impl Drop for OverlayGuard<'_, '_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for id in self.layer_ids.drain(..).rev() {
            self.style.remove_layer(&id);
        }
        for id in self.source_ids.drain(..).rev() {
            self.style.remove_source(&id);
        }
    }
}

fn add_fill(
    style: &mut Style<'_, Static>,
    layer_id: &str,
    source_id: &str,
    props: &OverlayProperties,
) -> Result<(), StyleError> {
    let mut layer = FillLayer::new(layer_id, source_id);
    if let Some(c) = props.fill_color {
        layer.set_fill_color(c.into());
    }
    if let Some(o) = props.fill_opacity {
        layer.set_fill_opacity(o);
    }
    if let Some(c) = props.fill_outline_color {
        layer.set_fill_outline_color(c.into());
    }
    style.add_layer(layer).map(|_| ())
}

fn add_line(
    style: &mut Style<'_, Static>,
    layer_id: &str,
    source_id: &str,
    props: &OverlayProperties,
) -> Result<(), StyleError> {
    let mut layer = LineLayer::new(layer_id, source_id);
    if let Some(c) = props.line_color {
        layer.set_line_color(c.into());
    }
    if let Some(o) = props.line_opacity {
        layer.set_line_opacity(o);
    }
    if let Some(w) = props.line_width {
        layer.set_line_width(w);
    }
    if let Some(cap) = props.line_cap {
        layer.set_line_cap(cap.into());
    }
    if let Some(join) = props.line_join {
        layer.set_line_join(join.into());
    }
    style.add_layer(layer).map(|_| ())
}

fn add_circle(
    style: &mut Style<'_, Static>,
    layer_id: &str,
    source_id: &str,
    props: &OverlayProperties,
) -> Result<(), StyleError> {
    let mut layer = CircleLayer::new(layer_id, source_id);
    if let Some(c) = props.circle_color {
        layer.set_circle_color(c.into());
    }
    if let Some(o) = props.circle_opacity {
        layer.set_circle_opacity(o);
    }
    if let Some(r) = props.circle_radius {
        layer.set_circle_radius(r);
    }
    if let Some(c) = props.circle_stroke_color {
        layer.set_circle_stroke_color(c.into());
    }
    if let Some(o) = props.circle_stroke_opacity {
        layer.set_circle_stroke_opacity(o);
    }
    if let Some(w) = props.circle_stroke_width {
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
