//! [`OverlaySpec`] -> sources and layers added to a renderer's map.

use geojson::{Geometry, GeometryValue};
use maplibre_native_ffi::MapHandle;
use serde_json::{Map, Value, json};

use crate::overlay::{ApplyError, Color, LineCap, LineJoin, OverlayFeature, OverlayProperties, OverlaySpec};
use crate::resources::styles::StyleError;
use crate::resources::styles::renderer::Renderer;

/// Prefix prepended to every synthetic source/layer id before it reaches maplibre.
/// Guarantees overlay ids cannot collide with the base style.
const ID_PREFIX: &str = "overlay:";

/// Handle to overlay sources and layers added to a renderer's map. Must be removed
/// from the map after the next render.
#[must_use = "AppliedOverlay must be removed from the style after rendering"]
#[derive(Debug)]
pub struct AppliedOverlay {
    layer_ids: Vec<String>,
    source_ids: Vec<String>,
}

impl AppliedOverlay {
    /// Remove every layer (in reverse-add order) then every source.
    /// Returns the style to a clean base before the map's next still image.
    pub fn remove_from(self, map: &MapHandle) {
        remove(map, self.layer_ids, self.source_ids);
    }
}

/// Queues the removal of `layer_ids` (in reverse-add order) then `source_ids`.
///
/// Commands apply in order, so they take effect before any later still image; a failed
/// removal only leaves an id that the next overlay replaces, so failures are not awaited.
fn remove(map: &MapHandle, layer_ids: Vec<String>, source_ids: Vec<String>) {
    for id in layer_ids.into_iter().rev() {
        let _ = map.discarding().remove_style_layer(&id);
    }
    for id in source_ids.into_iter().rev() {
        let _ = map.discarding().remove_style_source(&id);
    }
}

/// Adds `spec` to `renderer`'s map.
/// On any failure mid-way, rolls back what this call added (in reverse order) before returning `Err`.
///
/// # Errors
///
/// Returns [`StyleError::OverlayApply`] when maplibre rejects a source or layer, and the renderer's own error when a call fails otherwise.
pub fn apply_to_style(spec: &OverlaySpec, renderer: &Renderer) -> Result<AppliedOverlay, StyleError> {
    let mut guard = OverlayGuard::new(renderer);
    for (index, feature) in spec.features.iter().enumerate() {
        guard.apply_feature(index, feature)?;
    }
    Ok(guard.commit())
}

/// Which layer kinds a feature fans out to, in draw order:
/// - A point draws a circle,
/// - a line draws a line, and
/// - a polygon fills (unless only line properties are set) and outlines (when any line property is present)
/// - a bare polygon still fills so it stays visible.
/// - `None`/`GeometryCollection` geometries produce nothing and are skipped.
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
            let has_fill =
                props.fill_color.is_some() || props.fill_opacity.is_some() || props.fill_outline_color.is_some();
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

/// Scope guard for the apply pass: tracks the ids it adds and, unless [`commit`](OverlayGuard::commit)ted, removes them again on drop.
/// On success the ids move to an [`AppliedOverlay`] for removal after the render.
struct OverlayGuard<'r> {
    renderer: &'r Renderer,
    layer_ids: Vec<String>,
    source_ids: Vec<String>,
    committed: bool,
}

impl<'r> OverlayGuard<'r> {
    const fn new(renderer: &'r Renderer) -> Self {
        Self {
            renderer,
            layer_ids: Vec::new(),
            source_ids: Vec::new(),
            committed: false,
        }
    }

    fn apply_feature(&mut self, index: usize, feature: &OverlayFeature) -> Result<(), StyleError> {
        let props = feature.properties.clone().unwrap_or_default();
        let kinds = layer_kinds(feature.geometry.as_ref(), &props);
        if kinds.is_empty() {
            // null / unsupported geometry, or a geometry that draws nothing.
            return Ok(());
        }
        let geometry = feature
            .geometry
            .as_ref()
            .expect("layer_kinds is empty for a missing geometry");

        let source_id = format!("{ID_PREFIX}f{index}");
        let source = json!({"type": "geojson", "data": geometry});
        let map = self.renderer.map();
        self.renderer
            .command(map.add_style_source_json(&source_id, &source.to_string()))
            .map_err(|e| match e {
                StyleError::RenderingError(error) => StyleError::OverlayApply(ApplyError { id: format!("f{index}"), error }),
                e => e,
            })?;
        self.source_ids.push(source_id.clone());

        for kind in kinds {
            let layer_id = format!("{ID_PREFIX}f{index}-{kind}");
            let layer = layer_json(kind, &layer_id, &source_id, &props);
            self.renderer
                .command(map.add_style_layer_json(&layer.to_string(), ""))
                .map_err(|e| match e {
                    StyleError::RenderingError(error) => StyleError::OverlayApply(ApplyError { id: format!("f{index}-{kind}"), error }),
                    e => e,
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

impl Drop for OverlayGuard<'_> {
    fn drop(&mut self) {
        if !self.committed {
            remove(
                self.renderer.map(),
                std::mem::take(&mut self.layer_ids),
                std::mem::take(&mut self.source_ids),
            );
        }
    }
}

/// The style-spec layer drawing `source_id` as `kind` with the paint and layout `props` set.
fn layer_json(kind: LayerKind, layer_id: &str, source_id: &str, props: &OverlayProperties) -> Value {
    let mut paint = Map::new();
    let mut layout = Map::new();
    let set = |map: &mut Map<String, Value>, key: &str, value: Option<Value>| {
        if let Some(value) = value {
            map.insert(key.to_owned(), value);
        }
    };
    let color = |c: Option<Color>| c.map(color_json);
    let number = |n: Option<f32>| n.map(|n| json!(n));
    match kind {
        LayerKind::Fill => {
            set(&mut paint, "fill-color", color(props.fill_color));
            set(&mut paint, "fill-opacity", number(props.fill_opacity));
            set(&mut paint, "fill-outline-color", color(props.fill_outline_color));
        }
        LayerKind::Line => {
            set(&mut paint, "line-color", color(props.line_color));
            set(&mut paint, "line-opacity", number(props.line_opacity));
            set(&mut paint, "line-width", number(props.line_width));
            set(&mut layout, "line-cap", props.line_cap.map(|cap| json!(line_cap(cap))));
            set(
                &mut layout,
                "line-join",
                props.line_join.map(|join| json!(line_join(join))),
            );
        }
        LayerKind::Circle => {
            set(&mut paint, "circle-color", color(props.circle_color));
            set(&mut paint, "circle-opacity", number(props.circle_opacity));
            set(&mut paint, "circle-radius", number(props.circle_radius));
            set(&mut paint, "circle-stroke-color", color(props.circle_stroke_color));
            set(&mut paint, "circle-stroke-opacity", number(props.circle_stroke_opacity));
            set(&mut paint, "circle-stroke-width", number(props.circle_stroke_width));
        }
    }
    json!({"id": layer_id, "type": kind.to_string(), "source": source_id, "paint": paint, "layout": layout})
}

/// An `rgba` expression, which keeps fractional channels that a CSS color string would round.
fn color_json(c: Color) -> Value {
    json!(["rgba", c.r * 255.0, c.g * 255.0, c.b * 255.0, c.a])
}

/// The style-spec name of `cap`.
const fn line_cap(cap: LineCap) -> &'static str {
    match cap {
        LineCap::Butt => "butt",
        LineCap::Round => "round",
        LineCap::Square => "square",
    }
}

/// The style-spec name of `join`.
const fn line_join(join: LineJoin) -> &'static str {
    match join {
        LineJoin::Miter => "miter",
        LineJoin::Bevel => "bevel",
        LineJoin::Round => "round",
    }
}
