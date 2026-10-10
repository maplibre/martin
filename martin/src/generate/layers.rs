//! Lowers table sources and their `layers` into the tables of a [`martin_tilegen::plan::Plan`].

use std::ops::RangeInclusive;

use indexmap::IndexSet;
use martin_core::generate::props::Prop;
use martin_tilegen::plan::{
    AttributesDef, ColumnDef, ComputedAttr, GeometryType, IdDef, LayerDef, TableDef, ValueDef,
};
use martin_tilegen::{LayerGrid, MAX_ZOOM, PixelThreshold};
use tilejson::Bounds;
use tracing::warn;

use super::postgres::ScanTable;
use super::{GenerateError, GenerateResult};
use crate::config::file::postgres::TableInfo;
use crate::config::file::tiling::{
    Attributes, Columns, IdPolicy, Layer, Literal, OutputGeometry, PerFeature, Pixels,
    PropertySelector, Value, Zoom, ZoomSetting,
};

const DEFAULT_EXTENT: u32 = 4096;
const DEFAULT_BUFFER: u32 = 64;

#[derive(Clone, Debug)]
pub struct LowerOptions {
    pub zooms: RangeInclusive<u8>,
    /// WGS84 area to generate, if not the whole world.
    pub bounds: Option<Bounds>,
}

/// The table's layers, or one layer named by `layer_id` or `id` without `layers`. `None` when the
/// table, or every layer with fixed zooms, has no zoom within the options' zooms.
///
/// # Errors
///
/// Fails when a layer uses a setting `martin generate` does not support yet.
pub fn lower_table(
    id: &str,
    info: TableInfo,
    options: &LowerOptions,
) -> GenerateResult<Option<ScanTable>> {
    let (cli_min, cli_max) = (*options.zooms.start(), *options.zooms.end());
    let min = info.minzoom.unwrap_or(0).max(cli_min);
    let max = info.maxzoom.unwrap_or(u8::MAX).min(cli_max);
    if min > max {
        warn!("Skipping source `{id}`: it has no zoom between {cli_min} and {cli_max}");
        return Ok(None);
    }
    let defaults = LayerDef {
        bounds: options.bounds,
        ..LayerDef::new(
            String::new(),
            min..=max,
            LayerGrid {
                extent: info
                    .extent
                    .map_or(DEFAULT_EXTENT, std::num::NonZeroU32::get),
                buffer: info.buffer.unwrap_or(DEFAULT_BUFFER),
            },
        )
    };
    let properties: Vec<(&str, bool)> = info
        .properties
        .iter()
        .flatten()
        .map(|(column, label)| {
            let jsonb = info.column_type(column).unwrap_or(label) == "jsonb";
            (column.as_str(), jsonb)
        })
        .collect();
    let static_columns: Vec<&str> = properties
        .iter()
        .filter(|(_, jsonb)| !jsonb)
        .map(|(column, _)| *column)
        .collect();
    let layers = match info.layers.as_deref() {
        None => vec![LayerDef {
            name: info.layer_id.clone().unwrap_or_else(|| id.to_owned()),
            ..defaults
        }],
        Some(layers) => {
            let mut lowered = Vec::new();
            for (name, layer) in layers.iter() {
                if let Some(def) = lower_layer(id, name, layer, &defaults, &static_columns)? {
                    lowered.push(def);
                }
            }
            lowered
        }
    };
    if layers.is_empty() {
        return Ok(None);
    }
    let def = TableDef {
        columns: properties
            .into_iter()
            .map(|(column, jsonb)| {
                let column = column.to_owned();
                if jsonb {
                    ColumnDef::Keys(column)
                } else {
                    ColumnDef::Value(column)
                }
            })
            .collect(),
        layers,
    };
    Ok(Some(ScanTable { info, def }))
}

fn lower_layer(
    id: &str,
    name: &str,
    layer: &Layer,
    defaults: &LayerDef,
    static_columns: &[&str],
) -> GenerateResult<Option<LayerDef>> {
    if let Some(what) = unsupported(layer) {
        return Err(GenerateError::Unsupported {
            layer: name.to_owned(),
            what,
        });
    }
    let fixed = |zoom: Option<&ZoomSetting>| zoom.and_then(ZoomSetting::fixed).map(|z| z.get());
    let (table_min, table_max) = (*defaults.zooms.start(), *defaults.zooms.end());
    let min = fixed(layer.minzoom.as_ref()).map_or(table_min, |z| z.max(table_min));
    let max = fixed(layer.maxzoom.as_ref()).map_or(table_max, |z| z.min(table_max));
    if min > max {
        warn!(
            "Skipping layer `{name}` of source `{id}`: it has no zoom between {table_min} and {table_max}"
        );
        return Ok(None);
    }
    let threshold =
        |below: Option<Pixels>, at: Option<Pixels>, default: PixelThreshold| PixelThreshold {
            below_max_zoom: below.map_or(default.below_max_zoom, Pixels::get),
            at_max_zoom: at.map_or(default.at_max_zoom, Pixels::get),
        };
    Ok(Some(LayerDef {
        name: name.to_owned(),
        zooms: min..=max,
        grid: LayerGrid {
            extent: layer
                .extent
                .map_or(defaults.grid.extent, std::num::NonZeroU32::get),
            buffer: layer.buffer.unwrap_or(defaults.grid.buffer),
        },
        simplify: threshold(
            layer.simplify,
            layer.simplify_at_maxzoom(),
            PixelThreshold::DEFAULT_SIMPLIFY,
        ),
        min_size: threshold(
            layer.min_size,
            layer.min_size_at_maxzoom(),
            PixelThreshold::DEFAULT_MIN_SIZE,
        ),
        bounds: defaults.bounds,
        encoder: defaults.encoder,
        geometry: layer.geometry.and_then(|g| geometry_type(g).ok()),
        filter: layer.r#where.as_ref().map(|e| e.as_str().to_owned()),
        minzoom_expr: zoom_expr(layer.minzoom.as_ref()),
        maxzoom_expr: zoom_expr(layer.maxzoom.as_ref()),
        id: match &layer.id {
            IdPolicy::Keep => IdDef::Keep,
            IdPolicy::Drop => IdDef::Drop,
            IdPolicy::Expr(expr) => IdDef::Expr(expr.as_str().to_owned()),
        },
        attributes: match &layer.attributes {
            Attributes::None => AttributesDef::None,
            Attributes::Properties(selectors) => {
                AttributesDef::Columns(expand(selectors.as_slice(), static_columns))
            }
            Attributes::AllProperties => AttributesDef::All,
            Attributes::Columns(columns) => computed(columns),
        },
        rules: Vec::new(),
    }))
}

fn zoom_expr(zoom: Option<&ZoomSetting>) -> Option<String> {
    match zoom? {
        PerFeature::Fixed(_) => None,
        PerFeature::Expr(expr) => Some(expr.as_str().to_owned()),
    }
}

fn computed(columns: &Columns) -> AttributesDef {
    let attributes = columns
        .computed
        .iter()
        .map(|(name, spec)| ComputedAttr {
            name: name.clone(),
            value: match &spec.value {
                Value::Literal(Literal::Bool(v)) => ValueDef::Literal(Prop::Bool(*v)),
                Value::Literal(Literal::Int(v)) => ValueDef::Literal(Prop::I64(*v)),
                Value::Literal(Literal::Float(v)) => ValueDef::Literal(Prop::F64(v.get())),
                Value::Literal(Literal::String(v)) => ValueDef::Literal(Prop::Str(v.clone())),
                Value::Expr(expr) => ValueDef::Expr(expr.as_str().to_owned()),
            },
            zooms: spec.zooms.minzoom().map_or(0, Zoom::get)
                ..=spec.zooms.maxzoom().map_or(MAX_ZOOM, Zoom::get),
        })
        .collect();
    AttributesDef::Computed {
        attributes,
        prefixes: columns.copied_prefixes.clone(),
    }
}

fn geometry_type(geometry: OutputGeometry) -> Result<GeometryType, &'static str> {
    match geometry {
        OutputGeometry::Point => Ok(GeometryType::Point),
        OutputGeometry::Line => Ok(GeometryType::Line),
        OutputGeometry::Polygon => Ok(GeometryType::Polygon),
        OutputGeometry::LabelPoint => Err("label_point"),
        OutputGeometry::Centroid => Err("centroid"),
        OutputGeometry::CentroidIfConvex => Err("centroid_if_convex"),
        OutputGeometry::LineMidpoint => Err("line_midpoint"),
        OutputGeometry::InnermostPoint => Err("innermost_point"),
    }
}

fn unsupported(layer: &Layer) -> Option<String> {
    let geometry = layer.geometry.and_then(|g| geometry_type(g).err());
    let tile = &layer.tile;
    let tile_op = [
        ("merge_lines", tile.merge_lines.is_some()),
        ("merge_polygons", tile.merge_polygons.is_some()),
        ("merge_multi", tile.merge_multi.is_some()),
        ("label_grid", tile.label_grid.is_some()),
        ("dedup_by", tile.dedup_by.is_some()),
        ("limit", tile.limit.is_some()),
    ]
    .into_iter()
    .find_map(|(op, set)| set.then_some(op));
    if let Some(geometry) = geometry {
        Some(format!("`geometry: {geometry}`"))
    } else if layer.rules.is_some() {
        Some("`rules`".to_owned())
    } else if layer.sort_by.is_some() {
        Some("`sort_by`".to_owned())
    } else {
        tile_op.map(|op| format!("`tile: {op}`"))
    }
}

/// Named keys in their order, each prefix as the matching table columns in table order; keys inside
/// a `jsonb` column are only selected by name.
fn expand(selectors: &[PropertySelector], static_columns: &[&str]) -> Vec<String> {
    let mut keys = IndexSet::new();
    for selector in selectors {
        match selector {
            PropertySelector::Named(name) => {
                keys.insert(name.clone());
            }
            PropertySelector::Prefixed(prefix) => keys.extend(
                static_columns
                    .iter()
                    .filter(|column| column.starts_with(prefix.as_str()))
                    .map(|&column| column.to_owned()),
            ),
        }
    }
    keys.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    fn lower(yaml: &str, zooms: RangeInclusive<u8>) -> GenerateResult<Option<ScanTable>> {
        let info: TableInfo = serde_saphyr::from_str(yaml).expect("valid table");
        lower_table(
            "roads_source",
            info,
            &LowerOptions {
                zooms,
                bounds: None,
            },
        )
    }

    #[test]
    fn a_table_without_layers_is_one_layer_named_by_its_layer_id() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            layer_id: roads
            schema: public
            table: roads
            srid: 4326
            geometry_column: geom
            minzoom: 2
            maxzoom: 10
            extent: 512
            buffer: 8
            properties:
              name: text
              rank: int4
        "})
        .expect("valid table");
        let options = LowerOptions {
            zooms: 0..=8,
            bounds: Some(Bounds::new(-10.0, -5.0, 10.0, 5.0)),
        };
        let scan = lower_table("roads_source", info, &options)
            .expect("lowers")
            .expect("has a layer");
        let mut settings = insta::Settings::clone_current();
        settings.add_filter(r"EncoderConfig \{[^}]*\}", "EncoderConfig { .. }");
        let _guard = settings.bind_to_scope();
        insta::assert_debug_snapshot!(scan.def, @r#"
        TableDef {
            columns: [
                Value(
                    "name",
                ),
                Value(
                    "rank",
                ),
            ],
            layers: [
                LayerDef {
                    name: "roads",
                    zooms: 2..=8,
                    grid: LayerGrid {
                        extent: 512,
                        buffer: 8,
                    },
                    simplify: PixelThreshold {
                        below_max_zoom: 0.1,
                        at_max_zoom: 0.0625,
                    },
                    min_size: PixelThreshold {
                        below_max_zoom: 1.0,
                        at_max_zoom: 0.0625,
                    },
                    bounds: Some(
                        Bounds {
                            left: -10.0,
                            bottom: -5.0,
                            right: 10.0,
                            top: 5.0,
                        },
                    ),
                    encoder: EncoderConfig { .. },
                    geometry: None,
                    filter: None,
                    minzoom_expr: None,
                    maxzoom_expr: None,
                    id: Keep,
                    attributes: All,
                    rules: [],
                },
            ],
        }
        "#);
    }

    #[test]
    fn a_table_without_layer_id_names_its_layer_by_the_source_id() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
            "},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        let mut settings = insta::Settings::clone_current();
        settings.add_filter(r"EncoderConfig \{[^}]*\}", "EncoderConfig { .. }");
        let _guard = settings.bind_to_scope();
        insta::assert_debug_snapshot!(scan.def, @r#"
        TableDef {
            columns: [],
            layers: [
                LayerDef {
                    name: "roads_source",
                    zooms: 0..=14,
                    grid: LayerGrid {
                        extent: 4096,
                        buffer: 64,
                    },
                    simplify: PixelThreshold {
                        below_max_zoom: 0.1,
                        at_max_zoom: 0.0625,
                    },
                    min_size: PixelThreshold {
                        below_max_zoom: 1.0,
                        at_max_zoom: 0.0625,
                    },
                    bounds: None,
                    encoder: EncoderConfig { .. },
                    geometry: None,
                    filter: None,
                    minzoom_expr: None,
                    maxzoom_expr: None,
                    id: Keep,
                    attributes: All,
                    rules: [],
                },
            ],
        }
        "#);
    }

    #[test]
    #[expect(clippy::too_many_lines)]
    fn each_layer_overrides_the_table() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                minzoom: 1
                extent: 1024
                buffer: 16
                prefetch: 2
                properties:
                  name: text
                  class: text
                layers:
                  roads:
                    geometry: line
                    minzoom: 4
                    maxzoom: 20
                    extent: 4096
                    buffer: 0
                    simplify: 2
                    min_size_at_maxzoom: 0.5
                    id: false
                  road_points:
                    geometry: point
                    maxzoom: 6
                    simplify: 1
                    simplify_at_maxzoom: 0
                    attributes: []
                  areas:
                    geometry: polygon
                    min_size: 4
                    id: true
            "},
            0..=12,
        )
        .expect("lowers")
        .expect("has layers");
        let mut settings = insta::Settings::clone_current();
        settings.add_filter(r"EncoderConfig \{[^}]*\}", "EncoderConfig { .. }");
        let _guard = settings.bind_to_scope();
        insta::assert_debug_snapshot!(scan.def, @r#"
        TableDef {
            columns: [
                Value(
                    "class",
                ),
                Value(
                    "name",
                ),
            ],
            layers: [
                LayerDef {
                    name: "roads",
                    zooms: 4..=12,
                    grid: LayerGrid {
                        extent: 4096,
                        buffer: 0,
                    },
                    simplify: PixelThreshold {
                        below_max_zoom: 2.0,
                        at_max_zoom: 2.0,
                    },
                    min_size: PixelThreshold {
                        below_max_zoom: 1.0,
                        at_max_zoom: 0.5,
                    },
                    bounds: None,
                    encoder: EncoderConfig { .. },
                    geometry: Some(
                        Line,
                    ),
                    filter: None,
                    minzoom_expr: None,
                    maxzoom_expr: None,
                    id: Drop,
                    attributes: All,
                    rules: [],
                },
                LayerDef {
                    name: "road_points",
                    zooms: 1..=6,
                    grid: LayerGrid {
                        extent: 1024,
                        buffer: 16,
                    },
                    simplify: PixelThreshold {
                        below_max_zoom: 1.0,
                        at_max_zoom: 0.0,
                    },
                    min_size: PixelThreshold {
                        below_max_zoom: 1.0,
                        at_max_zoom: 0.0625,
                    },
                    bounds: None,
                    encoder: EncoderConfig { .. },
                    geometry: Some(
                        Point,
                    ),
                    filter: None,
                    minzoom_expr: None,
                    maxzoom_expr: None,
                    id: Keep,
                    attributes: None,
                    rules: [],
                },
                LayerDef {
                    name: "areas",
                    zooms: 1..=12,
                    grid: LayerGrid {
                        extent: 1024,
                        buffer: 16,
                    },
                    simplify: PixelThreshold {
                        below_max_zoom: 0.1,
                        at_max_zoom: 0.0625,
                    },
                    min_size: PixelThreshold {
                        below_max_zoom: 4.0,
                        at_max_zoom: 4.0,
                    },
                    bounds: None,
                    encoder: EncoderConfig { .. },
                    geometry: Some(
                        Polygon,
                    ),
                    filter: None,
                    minzoom_expr: None,
                    maxzoom_expr: None,
                    id: Keep,
                    attributes: All,
                    rules: [],
                },
            ],
        }
        "#);
    }

    #[test]
    fn attributes_select_columns_and_jsonb_keys() {
        let scan = lower(
            indoc! {r#"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                properties:
                  class: text
                  name: text
                  "name:de": text
                  "name:en": text
                  rank: int4
                  tags: jsonb
                layers:
                  labels:
                    attributes: [rank, "name:*", name, "name:de"]
                  kinds:
                    attributes: [class, surface]
                  prefixed_keys:
                    attributes: ["surf*"]
            "#},
            0..=14,
        )
        .expect("lowers")
        .expect("has layers");
        let attributes: Vec<_> = scan
            .def
            .layers
            .iter()
            .map(|layer| (layer.name.as_str(), &layer.attributes))
            .collect();
        insta::assert_debug_snapshot!(
            (&scan.def.columns, attributes),
            @r#"
        (
            [
                Value(
                    "class",
                ),
                Value(
                    "name",
                ),
                Value(
                    "name:de",
                ),
                Value(
                    "name:en",
                ),
                Value(
                    "rank",
                ),
                Keys(
                    "tags",
                ),
            ],
            [
                (
                    "labels",
                    Columns(
                        [
                            "rank",
                            "name:de",
                            "name:en",
                            "name",
                        ],
                    ),
                ),
                (
                    "kinds",
                    Columns(
                        [
                            "class",
                            "surface",
                        ],
                    ),
                ),
                (
                    "prefixed_keys",
                    Columns(
                        [],
                    ),
                ),
            ],
        )
        "#
        );
    }

    #[test]
    fn a_layer_outside_the_zooms_is_skipped() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                maxzoom: 10
                layers:
                  roads:
                    maxzoom: 8
                  buildings:
                    minzoom: 11
            "},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        let names: Vec<_> = scan.def.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["roads"]);
    }

    #[test]
    fn a_table_whose_layers_are_all_outside_the_zooms_is_dropped() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                layers:
                  buildings:
                    minzoom: 15
            "},
            0..=14,
        )
        .expect("lowers");
        assert!(scan.is_none());
    }

    #[test]
    fn a_table_outside_the_zooms_is_dropped() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                minzoom: 15
                layers:
                  roads: {}
            "},
            0..=14,
        )
        .expect("lowers");
        assert!(scan.is_none());
    }

    #[test]
    fn expressions_lower_into_the_layer() {
        let scan = lower(
            indoc! {r#"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                properties:
                  class: text
                  name: text
                  "name:en": text
                  rank: int4
                layers:
                  roads:
                    where: "class != 'service'"
                    minzoom: "rank > 3 ? 4 : 8"
                    maxzoom: 12
                    id: "rank * 10"
                    attributes:
                      kind: class
                      label: { expr: name, minzoom: 10 }
                      source: { value: osm }
                      lanes: 2
                      "name:*": "name:*"
            "#},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        let layer = &scan.def.layers[0];
        insta::assert_debug_snapshot!(
            (&scan.def.columns, &layer.zooms, &layer.filter, &layer.minzoom_expr, &layer.maxzoom_expr, &layer.id, &layer.attributes),
            @r#"
        (
            [
                Value(
                    "class",
                ),
                Value(
                    "name",
                ),
                Value(
                    "name:en",
                ),
                Value(
                    "rank",
                ),
            ],
            0..=12,
            Some(
                "class != 'service'",
            ),
            Some(
                "rank > 3 ? 4 : 8",
            ),
            None,
            Expr(
                "rank * 10",
            ),
            Computed {
                attributes: [
                    ComputedAttr {
                        name: "kind",
                        value: Expr(
                            "class",
                        ),
                        zooms: 0..=27,
                    },
                    ComputedAttr {
                        name: "label",
                        value: Expr(
                            "name",
                        ),
                        zooms: 10..=27,
                    },
                    ComputedAttr {
                        name: "source",
                        value: Literal(
                            Str(
                                "osm",
                            ),
                        ),
                        zooms: 0..=27,
                    },
                    ComputedAttr {
                        name: "lanes",
                        value: Literal(
                            I64(
                                2,
                            ),
                        ),
                        zooms: 0..=27,
                    },
                ],
                prefixes: [
                    "name:",
                ],
            },
        )
        "#
        );
    }

    #[test]
    fn unsupported_settings_are_rejected() {
        let layers = [
            "roads: { geometry: centroid }",
            "roads: { geometry: label_point }",
            "roads: { rules: [ { where: \"class == 'primary'\", minzoom: 4 } ] }",
            "roads: { sort_by: rank }",
            "roads: { tile: { merge_lines: { by: [class] } } }",
            "roads: { tile: { limit: 100 } }",
        ];
        let errors: Vec<String> = layers
            .iter()
            .map(|layers| {
                let yaml = format!(
                    "schema: public\ntable: roads\nsrid: 4326\ngeometry_column: geom\nlayers: {{ {layers} }}"
                );
                lower(&yaml, 0..=14)
                    .map(|_| ())
                    .expect_err(layers)
                    .to_string()
            })
            .collect();
        insta::assert_snapshot!(errors.join("\n"), @"
        layer `roads`: `geometry: centroid` is not supported by martin generate yet
        layer `roads`: `geometry: label_point` is not supported by martin generate yet
        layer `roads`: `rules` is not supported by martin generate yet
        layer `roads`: `sort_by` is not supported by martin generate yet
        layer `roads`: `tile: merge_lines` is not supported by martin generate yet
        layer `roads`: `tile: limit` is not supported by martin generate yet
        ");
    }
}
