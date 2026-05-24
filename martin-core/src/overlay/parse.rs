//! JSON → typed [`OverlaySpec`].
//!
//! Strict: every key is validated; unknown keys, expression arrays, and URL
//! `data` are rejected so that what callers send is exactly what maplibre
//! will render.

use std::collections::HashSet;

use csscolorparser::ParseColorError;
use serde_json::Value;

use crate::overlay::{
    CirclePaint, Color, FillPaint, LineCap, LineJoin, LineLayout, LinePaint, OverlayLayer,
    OverlaySource, OverlaySpec,
};

/// Errors produced while parsing an [`OverlaySpec`] from JSON.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum OverlayParseError {
    /// Top-level object contained an unknown key.
    #[error("unknown top-level key {0:?}; expected one of: sources, layers")]
    UnknownTopKey(String),

    /// `sources` was not a JSON object.
    #[error("`sources` must be a JSON object")]
    SourcesNotObject,

    /// `layers` was not a JSON array.
    #[error("`layers` must be a JSON array")]
    LayersNotArray,

    /// A source's `type` was not `"geojson"`.
    #[error("source {id:?}: only `type: geojson` is supported")]
    UnsupportedSourceType {
        /// Source id that triggered the error.
        id: String,
    },

    /// A source's `data` was not an inline JSON object (e.g. a URL string).
    #[error("source {id:?}: `data` must be an inline GeoJSON object, not a URL or string")]
    SourceDataMustBeInline {
        /// Source id that triggered the error.
        id: String,
    },

    /// A source's `data` failed to deserialize as `GeoJSON`.
    #[error("source {id:?}: malformed GeoJSON `data`: {source}")]
    MalformedGeoJson {
        /// Source id that triggered the error.
        id: String,
        /// Underlying deserialization error.
        #[source]
        source: serde_json::Error,
    },

    /// A layer's `type` was not one of `fill`, `line`, `circle`.
    #[error("layer {id:?}: unsupported `type` {ty:?}; expected one of: fill, line, circle")]
    UnsupportedLayerType {
        /// Layer id that triggered the error.
        id: String,
        /// The offending type value.
        ty: String,
    },

    /// A layer's `source` did not reference any parsed source.
    #[error("layer {id:?}: references unknown source {source_id:?}")]
    UnknownSource {
        /// Layer id that triggered the error.
        id: String,
        /// The unresolved source id.
        source_id: String,
    },

    /// A required field was missing or had the wrong type.
    #[error("layer {id:?}: missing or invalid required field {field:?}")]
    MissingField {
        /// Layer id that triggered the error (empty if missing from the layer object itself).
        id: String,
        /// The field name.
        field: &'static str,
    },

    /// A `paint`/`layout` object contained an unknown property.
    #[error("layer {id:?}: unknown {section} property {prop:?}")]
    UnknownLayerProp {
        /// Layer id that triggered the error.
        id: String,
        /// `"paint"` or `"layout"`.
        section: &'static str,
        /// The offending property name.
        prop: String,
    },

    /// A property's value was a JSON array (i.e. a data-driven expression),
    /// which this subset rejects.
    #[error("layer {id:?}: property {prop:?} is a data-driven expression; only literal values are supported")]
    ExpressionUnsupported {
        /// Layer id that triggered the error.
        id: String,
        /// The property name.
        prop: &'static str,
    },

    /// A color property's value did not parse as a CSS color.
    #[error("layer {id:?}: invalid CSS color for {prop:?}: {value:?} ({source})")]
    InvalidColor {
        /// Layer id that triggered the error.
        id: String,
        /// The property name.
        prop: &'static str,
        /// The raw value echoed back for diagnostics.
        value: String,
        /// The underlying `csscolorparser` error.
        #[source]
        source: ParseColorError,
    },

    /// A numeric property was non-numeric, non-finite, or otherwise out of band.
    #[error("layer {id:?}: invalid numeric value for {prop:?} (must be a finite number)")]
    InvalidNumber {
        /// Layer id that triggered the error.
        id: String,
        /// The property name.
        prop: &'static str,
    },

    /// `line-cap` was set but not one of `butt`, `round`, `square`.
    #[error("layer {id:?}: invalid `line-cap`; expected one of: butt, round, square")]
    InvalidLineCap {
        /// Layer id that triggered the error.
        id: String,
    },

    /// `line-join` was set but not one of `miter`, `bevel`, `round`.
    #[error("layer {id:?}: invalid `line-join`; expected one of: miter, bevel, round")]
    InvalidLineJoin {
        /// Layer id that triggered the error.
        id: String,
    },

    /// A source or layer id was declared twice.
    #[error("duplicate {kind} id {id:?}")]
    DuplicateId {
        /// `"source"` or `"layer"`.
        kind: &'static str,
        /// The duplicated id.
        id: String,
    },
}

/// Parse a JSON tree into an [`OverlaySpec`].
///
/// Every key is validated; unknown keys, expressions, and URL `data` are
/// rejected so the wire format exactly mirrors what maplibre will render.
///
/// # Errors
///
/// Returns [`OverlayParseError`] on any unknown key, malformed value, or
/// missing required field.
pub fn parse_spec(body: &Value) -> Result<OverlaySpec, OverlayParseError> {
    let Some(obj) = body.as_object() else {
        return Err(OverlayParseError::UnknownTopKey(format!("(not an object: {body})")));
    };

    let mut sources_val: Option<&Value> = None;
    let mut layers_val: Option<&Value> = None;
    for (k, v) in obj {
        match k.as_str() {
            "sources" => sources_val = Some(v),
            "layers" => layers_val = Some(v),
            other => return Err(OverlayParseError::UnknownTopKey(other.to_string())),
        }
    }

    let sources = match sources_val {
        Some(v) => parse_sources(v)?,
        None => Vec::new(),
    };
    let layers = match layers_val {
        Some(v) => parse_layers(v, &sources)?,
        None => Vec::new(),
    };

    Ok(OverlaySpec { sources, layers })
}

fn parse_sources(value: &Value) -> Result<Vec<OverlaySource>, OverlayParseError> {
    let obj = value
        .as_object()
        .ok_or(OverlayParseError::SourcesNotObject)?;
    let mut out = Vec::with_capacity(obj.len());
    let mut seen = HashSet::with_capacity(obj.len());
    for (id, src) in obj {
        if !seen.insert(id.clone()) {
            return Err(OverlayParseError::DuplicateId {
                kind: "source",
                id: id.clone(),
            });
        }
        out.push(parse_source(id, src)?);
    }
    Ok(out)
}

fn parse_source(id: &str, value: &Value) -> Result<OverlaySource, OverlayParseError> {
    let obj = value.as_object().ok_or_else(|| {
        OverlayParseError::MissingField {
            id: id.to_string(),
            field: "(source must be a JSON object)",
        }
    })?;

    let mut ty: Option<&str> = None;
    let mut data: Option<&Value> = None;
    for (k, v) in obj {
        match k.as_str() {
            "type" => {
                ty = Some(v.as_str().ok_or(OverlayParseError::UnsupportedSourceType {
                    id: id.to_string(),
                })?);
            }
            "data" => data = Some(v),
            other => {
                return Err(OverlayParseError::UnknownLayerProp {
                    id: id.to_string(),
                    section: "source",
                    prop: other.to_string(),
                });
            }
        }
    }

    if ty != Some("geojson") {
        return Err(OverlayParseError::UnsupportedSourceType {
            id: id.to_string(),
        });
    }
    let data = data.ok_or(OverlayParseError::MissingField {
        id: id.to_string(),
        field: "data",
    })?;
    if !data.is_object() {
        return Err(OverlayParseError::SourceDataMustBeInline {
            id: id.to_string(),
        });
    }

    let geojson: geojson::GeoJson = serde_json::from_value(data.clone()).map_err(|source| {
        OverlayParseError::MalformedGeoJson {
            id: id.to_string(),
            source,
        }
    })?;

    Ok(OverlaySource {
        id: id.to_string(),
        data: geojson,
    })
}

fn parse_layers(
    value: &Value,
    sources: &[OverlaySource],
) -> Result<Vec<OverlayLayer>, OverlayParseError> {
    let arr = value.as_array().ok_or(OverlayParseError::LayersNotArray)?;
    let mut out = Vec::with_capacity(arr.len());
    let mut seen = HashSet::with_capacity(arr.len());
    for layer in arr {
        let parsed = parse_layer(layer, sources)?;
        let id = layer_id(&parsed).to_string();
        if !seen.insert(id.clone()) {
            return Err(OverlayParseError::DuplicateId { kind: "layer", id });
        }
        out.push(parsed);
    }
    Ok(out)
}

fn layer_id(layer: &OverlayLayer) -> &str {
    match layer {
        OverlayLayer::Fill { id, .. }
        | OverlayLayer::Line { id, .. }
        | OverlayLayer::Circle { id, .. } => id,
    }
}

fn parse_layer(
    value: &Value,
    sources: &[OverlaySource],
) -> Result<OverlayLayer, OverlayParseError> {
    let obj = value.as_object().ok_or(OverlayParseError::MissingField {
        id: String::new(),
        field: "(layer must be a JSON object)",
    })?;

    // Pull out the always-present fields first so we know the id for any
    // subsequent error.
    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .ok_or(OverlayParseError::MissingField {
            id: String::new(),
            field: "id",
        })?
        .to_string();
    let ty = obj
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| OverlayParseError::MissingField {
            id: id.clone(),
            field: "type",
        })?;
    let source = obj
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| OverlayParseError::MissingField {
            id: id.clone(),
            field: "source",
        })?
        .to_string();
    if !sources.iter().any(|s| s.id == source) {
        return Err(OverlayParseError::UnknownSource {
            id,
            source_id: source,
        });
    }
    let before = match obj.get("before") {
        None => None,
        Some(v) => Some(
            v.as_str()
                .ok_or_else(|| OverlayParseError::MissingField {
                    id: id.clone(),
                    field: "before",
                })?
                .to_string(),
        ),
    };

    // Validate that the layer object has no unknown top-level keys.
    for k in obj.keys() {
        match k.as_str() {
            "id" | "type" | "source" | "before" | "paint" | "layout" => {}
            other => {
                return Err(OverlayParseError::UnknownLayerProp {
                    id: id.clone(),
                    section: "layer",
                    prop: other.to_string(),
                });
            }
        }
    }

    let paint_obj = obj.get("paint");
    let layout_obj = obj.get("layout");

    match ty {
        "fill" => {
            if layout_obj.is_some() {
                return Err(OverlayParseError::UnknownLayerProp {
                    id: id.clone(),
                    section: "layer",
                    prop: "layout".to_string(),
                });
            }
            let paint = parse_fill_paint(&id, paint_obj)?;
            Ok(OverlayLayer::Fill {
                id,
                source,
                before,
                paint,
            })
        }
        "line" => {
            let paint = parse_line_paint(&id, paint_obj)?;
            let layout = parse_line_layout(&id, layout_obj)?;
            Ok(OverlayLayer::Line {
                id,
                source,
                before,
                paint,
                layout,
            })
        }
        "circle" => {
            if layout_obj.is_some() {
                return Err(OverlayParseError::UnknownLayerProp {
                    id: id.clone(),
                    section: "layer",
                    prop: "layout".to_string(),
                });
            }
            let paint = parse_circle_paint(&id, paint_obj)?;
            Ok(OverlayLayer::Circle {
                id,
                source,
                before,
                paint,
            })
        }
        other => Err(OverlayParseError::UnsupportedLayerType {
            id,
            ty: other.to_string(),
        }),
    }
}

fn parse_fill_paint(
    id: &str,
    paint: Option<&Value>,
) -> Result<FillPaint, OverlayParseError> {
    let mut out = FillPaint::default();
    let Some(paint) = paint else { return Ok(out); };
    let obj = paint.as_object().ok_or(OverlayParseError::MissingField {
        id: id.to_string(),
        field: "(paint must be a JSON object)",
    })?;
    for (k, v) in obj {
        match k.as_str() {
            "fill-color" => out.color = Some(parse_color(id, "fill-color", v)?),
            "fill-opacity" => out.opacity = Some(parse_finite_f32(id, "fill-opacity", v)?),
            "fill-outline-color" => {
                out.outline_color = Some(parse_color(id, "fill-outline-color", v)?);
            }
            other => {
                return Err(OverlayParseError::UnknownLayerProp {
                    id: id.to_string(),
                    section: "paint",
                    prop: other.to_string(),
                });
            }
        }
    }
    Ok(out)
}

fn parse_line_paint(
    id: &str,
    paint: Option<&Value>,
) -> Result<LinePaint, OverlayParseError> {
    let mut out = LinePaint::default();
    let Some(paint) = paint else { return Ok(out); };
    let obj = paint.as_object().ok_or(OverlayParseError::MissingField {
        id: id.to_string(),
        field: "(paint must be a JSON object)",
    })?;
    for (k, v) in obj {
        match k.as_str() {
            "line-color" => out.color = Some(parse_color(id, "line-color", v)?),
            "line-opacity" => out.opacity = Some(parse_finite_f32(id, "line-opacity", v)?),
            "line-width" => out.width = Some(parse_finite_f32(id, "line-width", v)?),
            other => {
                return Err(OverlayParseError::UnknownLayerProp {
                    id: id.to_string(),
                    section: "paint",
                    prop: other.to_string(),
                });
            }
        }
    }
    Ok(out)
}

fn parse_line_layout(
    id: &str,
    layout: Option<&Value>,
) -> Result<LineLayout, OverlayParseError> {
    let mut out = LineLayout::default();
    let Some(layout) = layout else { return Ok(out); };
    let obj = layout.as_object().ok_or(OverlayParseError::MissingField {
        id: id.to_string(),
        field: "(layout must be a JSON object)",
    })?;
    for (k, v) in obj {
        match k.as_str() {
            "line-cap" => {
                reject_expression(id, "line-cap", v)?;
                out.cap = Some(match v.as_str() {
                    Some("butt") => LineCap::Butt,
                    Some("round") => LineCap::Round,
                    Some("square") => LineCap::Square,
                    _ => return Err(OverlayParseError::InvalidLineCap { id: id.to_string() }),
                });
            }
            "line-join" => {
                reject_expression(id, "line-join", v)?;
                out.join = Some(match v.as_str() {
                    Some("miter") => LineJoin::Miter,
                    Some("bevel") => LineJoin::Bevel,
                    Some("round") => LineJoin::Round,
                    _ => return Err(OverlayParseError::InvalidLineJoin { id: id.to_string() }),
                });
            }
            other => {
                return Err(OverlayParseError::UnknownLayerProp {
                    id: id.to_string(),
                    section: "layout",
                    prop: other.to_string(),
                });
            }
        }
    }
    Ok(out)
}

fn parse_circle_paint(
    id: &str,
    paint: Option<&Value>,
) -> Result<CirclePaint, OverlayParseError> {
    let mut out = CirclePaint::default();
    let Some(paint) = paint else { return Ok(out); };
    let obj = paint.as_object().ok_or(OverlayParseError::MissingField {
        id: id.to_string(),
        field: "(paint must be a JSON object)",
    })?;
    for (k, v) in obj {
        match k.as_str() {
            "circle-color" => out.color = Some(parse_color(id, "circle-color", v)?),
            "circle-opacity" => out.opacity = Some(parse_finite_f32(id, "circle-opacity", v)?),
            "circle-radius" => out.radius = Some(parse_finite_f32(id, "circle-radius", v)?),
            "circle-stroke-color" => {
                out.stroke_color = Some(parse_color(id, "circle-stroke-color", v)?);
            }
            "circle-stroke-opacity" => {
                out.stroke_opacity = Some(parse_finite_f32(id, "circle-stroke-opacity", v)?);
            }
            "circle-stroke-width" => {
                out.stroke_width = Some(parse_finite_f32(id, "circle-stroke-width", v)?);
            }
            other => {
                return Err(OverlayParseError::UnknownLayerProp {
                    id: id.to_string(),
                    section: "paint",
                    prop: other.to_string(),
                });
            }
        }
    }
    Ok(out)
}

fn parse_color(id: &str, prop: &'static str, value: &Value) -> Result<Color, OverlayParseError> {
    reject_expression(id, prop, value)?;
    let s = value.as_str().ok_or(OverlayParseError::InvalidColor {
        id: id.to_string(),
        prop,
        value: value.to_string(),
        source: csscolorparser::parse("")
            .expect_err("empty string never parses as a CSS color"),
    })?;
    let parsed = csscolorparser::parse(s).map_err(|source| OverlayParseError::InvalidColor {
        id: id.to_string(),
        prop,
        value: s.to_string(),
        source,
    })?;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "CSS colors fit in f32 with negligible precision loss"
    )]
    Ok(Color {
        r: parsed.r as f32,
        g: parsed.g as f32,
        b: parsed.b as f32,
        a: parsed.a as f32,
    })
}

fn parse_finite_f32(
    id: &str,
    prop: &'static str,
    value: &Value,
) -> Result<f32, OverlayParseError> {
    reject_expression(id, prop, value)?;
    let n = value.as_f64().ok_or(OverlayParseError::InvalidNumber {
        id: id.to_string(),
        prop,
    })?;
    if !n.is_finite() {
        return Err(OverlayParseError::InvalidNumber {
            id: id.to_string(),
            prop,
        });
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "downcast to f32 for the maplibre paint API"
    )]
    Ok(n as f32)
}

fn reject_expression(
    id: &str,
    prop: &'static str,
    value: &Value,
) -> Result<(), OverlayParseError> {
    if value.is_array() {
        return Err(OverlayParseError::ExpressionUnsupported {
            id: id.to_string(),
            prop,
        });
    }
    Ok(())
}
