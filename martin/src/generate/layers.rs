//! Lowers table sources and their `layers` into the tables of a [`martin_tilegen::plan::Plan`].

use std::collections::BTreeSet;
use std::ops::RangeInclusive;

use indexmap::IndexMap;
use martin_tilegen::expr::CompiledExpr;
use martin_tilegen::plan::{
    AttributesDef, ComputedAttr, GeometryType, IdDef, LayerDef, RuleDef, SortDef, TableDef,
    ValueDef, ZoomDef,
};
use martin_tilegen::props::Prop;
use martin_tilegen::{LayerGrid, MAX_ZOOM, PixelThreshold, TileGenError};
use tracing::warn;

use super::postgres::ScanTable;
use super::{GenerateError, GenerateResult};
use crate::config::file::postgres::TableInfo;
use crate::config::file::tiling::{
    Attributes, Columns, Expr, IdPolicy, Layer, Literal, OutputGeometry, PerFeature, Pixels,
    PropertySelector, RuleSettings, Rules, Value, ValueSpec, Zoom, ZoomSetting,
};

const DEFAULT_EXTENT: u32 = 4096;
const DEFAULT_BUFFER: u32 = 64;

#[derive(Clone, Debug)]
pub struct LowerOptions {
    pub zooms: RangeInclusive<u8>,
    /// WGS84 `[min_lon, min_lat, max_lon, max_lat]` to generate, if not the whole world.
    pub bbox: Option<[f64; 4]>,
}

/// The table's layers, or one layer named by `layer_id` or `id` without `layers`. `None` when every
/// layer has fixed zooms outside the table's and the options' zooms.
///
/// # Errors
///
/// Fails when the table has no zoom within the options' zooms, or a layer uses a setting `martin
/// generate` does not support yet.
pub fn lower_table(
    id: &str,
    info: TableInfo,
    options: &LowerOptions,
) -> GenerateResult<Option<ScanTable>> {
    let (cli_min, cli_max) = (*options.zooms.start(), *options.zooms.end());
    let min = info.minzoom.unwrap_or(0).max(cli_min);
    let max = info.maxzoom.unwrap_or(u8::MAX).min(cli_max);
    if min > max {
        return Err(GenerateError::NoZooms(id.to_owned(), cli_min, cli_max));
    }
    let defaults = LayerDef {
        clip: info.clip_geom.unwrap_or(true),
        bounds: options.bbox,
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
    let reads = expression_reads(&layers, &static_columns, properties.iter().any(|p| p.1))?;
    let selected: Vec<(&str, bool)> = properties
        .into_iter()
        .filter(|&(column, jsonb)| {
            let read = if jsonb {
                reads.other_keys
            } else {
                reads.columns.contains(column)
            };
            read || layers.iter().any(|layer| match &layer.attributes {
                AttributesDef::All => true,
                AttributesDef::None => false,
                AttributesDef::Columns(keys) if jsonb => keys
                    .iter()
                    .any(|key| !static_columns.contains(&key.as_str())),
                AttributesDef::Columns(keys) => keys.iter().any(|key| key == column),
                AttributesDef::Computed { .. } if jsonb => false,
                AttributesDef::Computed { prefixes, .. } => {
                    prefixes.iter().any(|p| column.starts_with(p.as_str()))
                }
            })
        })
        .collect();
    let table = TableDef {
        columns: selected.iter().map(|(c, _)| (*c).to_owned()).collect(),
        dynamic_props: selected.iter().any(|(_, jsonb)| *jsonb),
        layers,
    };
    Ok(Some(ScanTable { info, table }))
}

#[derive(Default)]
struct Reads<'a> {
    columns: BTreeSet<&'a str>,
    /// Whether some expression reads a key that is not a column, e.g. one inside a `jsonb` column.
    other_keys: bool,
}

/// What the layers' expressions read, which the scan must keep.
fn expression_reads<'a>(
    layers: &[LayerDef],
    static_columns: &[&'a str],
    dynamic_props: bool,
) -> GenerateResult<Reads<'a>> {
    let mut reads = Reads::default();
    for layer in layers {
        for source in layer.expressions() {
            let expr =
                CompiledExpr::compile(source, static_columns, dynamic_props).map_err(|error| {
                    TileGenError::LayerExpr {
                        layer: layer.name.clone(),
                        error,
                    }
                })?;
            reads.other_keys |= expr.needs_all_keys();
            for name in expr.columns() {
                match static_columns.iter().find(|column| **column == name) {
                    Some(column) => {
                        reads.columns.insert(column);
                    }
                    None => reads.other_keys = true,
                }
            }
        }
    }
    Ok(reads)
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
        clip: layer.clip_geom.unwrap_or(defaults.clip),
        simplify: threshold(
            layer.simplify,
            layer.simplify_at_maxzoom(),
            PixelThreshold::PLANETILER_SIMPLIFY,
        ),
        min_size: threshold(
            layer.min_size,
            layer.min_size_at_maxzoom(),
            PixelThreshold::PLANETILER_MIN_SIZE,
        ),
        bounds: defaults.bounds,
        order: defaults.order,
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
        rules: layer.rules.as_ref().map_or_else(Vec::new, lower_rules),
        sort_by: layer
            .sort_by
            .iter()
            .flatten()
            .map(|key| SortDef {
                expr: key.expr.as_str().to_owned(),
                descending: key.descending,
            })
            .collect(),
    }))
}

/// The rules in order, then the catch-all; a rule's `simplify` and `min_size` also apply at the max zoom.
fn lower_rules(rules: &Rules) -> Vec<RuleDef> {
    let lower = |when: Option<&Expr>, settings: &RuleSettings| {
        let zoom = |zoom: Option<&ZoomSetting>| {
            zoom.map(|zoom| match zoom {
                PerFeature::Fixed(zoom) => ZoomDef::Fixed(zoom.get()),
                PerFeature::Expr(expr) => ZoomDef::Expr(expr.as_str().to_owned()),
            })
        };
        let threshold = |pixels: Option<Pixels>| {
            pixels.map(|pixels| PixelThreshold {
                below_max_zoom: pixels.get(),
                at_max_zoom: pixels.get(),
            })
        };
        RuleDef {
            when: when.map(|expr| expr.as_str().to_owned()),
            minzoom: zoom(settings.minzoom.as_ref()),
            maxzoom: zoom(settings.maxzoom.as_ref()),
            simplify: threshold(settings.simplify),
            min_size: threshold(settings.min_size),
            attributes: computed_attrs(&settings.attributes),
        }
    };
    rules
        .cases
        .iter()
        .map(|rule| lower(Some(&rule.when), &rule.settings))
        .chain(rules.fallback.iter().map(|settings| lower(None, settings)))
        .collect()
}

fn zoom_expr(zoom: Option<&ZoomSetting>) -> Option<String> {
    match zoom? {
        PerFeature::Fixed(_) => None,
        PerFeature::Expr(expr) => Some(expr.as_str().to_owned()),
    }
}

fn computed(columns: &Columns) -> AttributesDef {
    AttributesDef::Computed {
        attributes: computed_attrs(&columns.computed),
        prefixes: columns.copied_prefixes.clone(),
    }
}

fn computed_attrs(attributes: &IndexMap<String, ValueSpec>) -> Vec<ComputedAttr> {
    attributes
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
        .collect()
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
    } else {
        tile_op.map(|op| format!("`tile: {op}`"))
    }
}

/// Named keys in their order, each prefix as the matching table columns in table order; keys inside
/// a `jsonb` column are only selected by name.
fn expand(selectors: &[PropertySelector], static_columns: &[&str]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for selector in selectors {
        let matched: Vec<&str> = match selector {
            PropertySelector::Named(name) => vec![name.as_str()],
            PropertySelector::Prefixed(prefix) => static_columns
                .iter()
                .copied()
                .filter(|column| column.starts_with(prefix.as_str()))
                .collect(),
        };
        for key in matched {
            if !keys.iter().any(|k| k == key) {
                keys.push(key.to_owned());
            }
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::config::file::postgres::resolver::scan_sql;

    fn lower(yaml: &str, zooms: RangeInclusive<u8>) -> GenerateResult<Option<ScanTable>> {
        let info: TableInfo = serde_saphyr::from_str(yaml).expect("valid table");
        lower_table("roads_source", info, &LowerOptions { zooms, bbox: None })
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
            clip_geom: false
            properties:
              name: text
              rank: int4
        "})
        .expect("valid table");
        let options = LowerOptions {
            zooms: 0..=8,
            bbox: Some([-10.0, -5.0, 10.0, 5.0]),
        };
        let scan = lower_table("roads_source", info, &options)
            .expect("lowers")
            .expect("has a layer");
        insta::assert_debug_snapshot!(scan.table, @r#"
        TableDef {
            columns: [
                "name",
                "rank",
            ],
            dynamic_props: false,
            layers: [
                LayerDef {
                    name: "roads",
                    zooms: 2..=8,
                    grid: LayerGrid {
                        extent: 512,
                        buffer: 8,
                    },
                    clip: false,
                    simplify: PixelThreshold {
                        below_max_zoom: 0.1,
                        at_max_zoom: 0.0625,
                    },
                    min_size: PixelThreshold {
                        below_max_zoom: 1.0,
                        at_max_zoom: 0.0625,
                    },
                    bounds: Some(
                        [
                            -10.0,
                            -5.0,
                            10.0,
                            5.0,
                        ],
                    ),
                    order: Source,
                    geometry: None,
                    filter: None,
                    minzoom_expr: None,
                    maxzoom_expr: None,
                    id: Keep,
                    attributes: All,
                    rules: [],
                    sort_by: [],
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
        insta::assert_debug_snapshot!(scan.table, @r#"
        TableDef {
            columns: [],
            dynamic_props: false,
            layers: [
                LayerDef {
                    name: "roads_source",
                    zooms: 0..=14,
                    grid: LayerGrid {
                        extent: 4096,
                        buffer: 64,
                    },
                    clip: true,
                    simplify: PixelThreshold {
                        below_max_zoom: 0.1,
                        at_max_zoom: 0.0625,
                    },
                    min_size: PixelThreshold {
                        below_max_zoom: 1.0,
                        at_max_zoom: 0.0625,
                    },
                    bounds: None,
                    order: Source,
                    geometry: None,
                    filter: None,
                    minzoom_expr: None,
                    maxzoom_expr: None,
                    id: Keep,
                    attributes: All,
                    rules: [],
                    sort_by: [],
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
                    clip_geom: false
                    simplify: 2
                    min_size_at_maxzoom: 0.5
                    id: drop
                  road_points:
                    geometry: point
                    maxzoom: 6
                    simplify: 1
                    simplify_at_maxzoom: 0
                    attributes: []
                  areas:
                    geometry: polygon
                    min_size: 4
                    id: keep
            "},
            0..=12,
        )
        .expect("lowers")
        .expect("has layers");
        insta::assert_debug_snapshot!(scan.table, @r#"
        TableDef {
            columns: [
                "class",
                "name",
            ],
            dynamic_props: false,
            layers: [
                LayerDef {
                    name: "roads",
                    zooms: 4..=12,
                    grid: LayerGrid {
                        extent: 4096,
                        buffer: 0,
                    },
                    clip: false,
                    simplify: PixelThreshold {
                        below_max_zoom: 2.0,
                        at_max_zoom: 2.0,
                    },
                    min_size: PixelThreshold {
                        below_max_zoom: 1.0,
                        at_max_zoom: 0.5,
                    },
                    bounds: None,
                    order: Source,
                    geometry: Some(
                        Line,
                    ),
                    filter: None,
                    minzoom_expr: None,
                    maxzoom_expr: None,
                    id: Drop,
                    attributes: All,
                    rules: [],
                    sort_by: [],
                },
                LayerDef {
                    name: "road_points",
                    zooms: 1..=6,
                    grid: LayerGrid {
                        extent: 1024,
                        buffer: 16,
                    },
                    clip: true,
                    simplify: PixelThreshold {
                        below_max_zoom: 1.0,
                        at_max_zoom: 0.0,
                    },
                    min_size: PixelThreshold {
                        below_max_zoom: 1.0,
                        at_max_zoom: 0.0625,
                    },
                    bounds: None,
                    order: Source,
                    geometry: Some(
                        Point,
                    ),
                    filter: None,
                    minzoom_expr: None,
                    maxzoom_expr: None,
                    id: Keep,
                    attributes: None,
                    rules: [],
                    sort_by: [],
                },
                LayerDef {
                    name: "areas",
                    zooms: 1..=12,
                    grid: LayerGrid {
                        extent: 1024,
                        buffer: 16,
                    },
                    clip: true,
                    simplify: PixelThreshold {
                        below_max_zoom: 0.1,
                        at_max_zoom: 0.0625,
                    },
                    min_size: PixelThreshold {
                        below_max_zoom: 4.0,
                        at_max_zoom: 4.0,
                    },
                    bounds: None,
                    order: Source,
                    geometry: Some(
                        Polygon,
                    ),
                    filter: None,
                    minzoom_expr: None,
                    maxzoom_expr: None,
                    id: Keep,
                    attributes: All,
                    rules: [],
                    sort_by: [],
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
            .table
            .layers
            .iter()
            .map(|layer| (layer.name.as_str(), &layer.attributes))
            .collect();
        insta::assert_debug_snapshot!(
            (&scan.table.columns, scan.table.dynamic_props, attributes),
            @r#"
        (
            [
                "class",
                "name",
                "name:de",
                "name:en",
                "rank",
                "tags",
            ],
            true,
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
    fn only_the_columns_some_layer_needs_are_scanned() {
        let scan = lower(
            indoc! {r#"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                id_column: gid
                properties:
                  class: text
                  name: text
                  rank: int4
                  tags: jsonb
                layers:
                  roads:
                    attributes: [rank]
                  road_labels:
                    attributes: ["na*"]
            "#},
            0..=14,
        )
        .expect("lowers")
        .expect("has layers");
        let sql = scan_sql(&scan.info, &scan.table.columns, None).expect("valid SQL");
        assert!(!scan.table.dynamic_props);
        insta::assert_snapshot!(sql.sql, @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))), "gid", "name", "rank" FROM "public"."roads" WHERE "geom" IS NOT NULL"#);
    }

    #[test]
    fn layers_without_attributes_scan_no_properties() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                properties:
                  name: text
                  tags: jsonb
                layers:
                  roads:
                    attributes: []
                  road_shapes:
                    attributes: []
            "},
            0..=14,
        )
        .expect("lowers")
        .expect("has layers");
        let sql = scan_sql(&scan.info, &scan.table.columns, None).expect("valid SQL");
        assert!(!scan.table.dynamic_props);
        insta::assert_snapshot!(sql.sql, @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))) FROM "public"."roads" WHERE "geom" IS NOT NULL"#);
    }

    #[test]
    fn a_table_without_layers_scans_every_property() {
        let scan = lower(
            indoc! {"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                properties:
                  name: text
                  tags: jsonb
            "},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        let sql = scan_sql(&scan.info, &scan.table.columns, None).expect("valid SQL");
        assert!(scan.table.dynamic_props);
        insta::assert_snapshot!(sql.sql, @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))), "name", "tags" FROM "public"."roads" WHERE "geom" IS NOT NULL"#);
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
        let names: Vec<_> = scan.table.layers.iter().map(|l| l.name.as_str()).collect();
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
    fn a_table_outside_the_zooms_is_rejected() {
        let err = lower(
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
        .expect_err("no zooms");
        insta::assert_snapshot!(err, @"Source `roads_source` has no zoom between 0 and 14");
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
                    id: { expr: "rank * 10" }
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
        let layer = &scan.table.layers[0];
        insta::assert_debug_snapshot!(
            (&scan.table.columns, &layer.zooms, &layer.filter, &layer.minzoom_expr, &layer.maxzoom_expr, &layer.id, &layer.attributes),
            @r#"
        (
            [
                "class",
                "name",
                "name:en",
                "rank",
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
    fn expressions_keep_the_columns_they_read() {
        let scan = lower(
            indoc! {r#"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                properties:
                  class: text
                  name: text
                  rank: int4
                  surface: text
                  tags: jsonb
                layers:
                  roads:
                    where: "class == 'primary'"
                    minzoom: "rank > 3 ? 4 : 8"
                    attributes:
                      label: { expr: "name", minzoom: 10 }
            "#},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        let sql = scan_sql(&scan.info, &scan.table.columns, None).expect("valid SQL");
        assert!(!scan.table.dynamic_props);
        insta::assert_snapshot!(sql.sql, @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))), "class", "name", "rank" FROM "public"."roads" WHERE "geom" IS NOT NULL"#);
    }

    #[test]
    fn an_expression_reading_a_key_outside_the_columns_keeps_the_jsonb_column() {
        let scan = lower(
            indoc! {r#"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                properties:
                  class: text
                  name: text
                  tags: jsonb
                layers:
                  roads:
                    where: "oneway == true"
                    attributes: []
            "#},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        let sql = scan_sql(&scan.info, &scan.table.columns, None).expect("valid SQL");
        assert!(scan.table.dynamic_props);
        insta::assert_snapshot!(sql.sql, @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))), "tags" FROM "public"."roads" WHERE "geom" IS NOT NULL"#);
    }

    #[test]
    fn an_expression_reading_an_unknown_property_is_rejected() {
        let err = lower(
            indoc! {r#"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                properties:
                  class: text
                layers:
                  roads:
                    attributes:
                      kind: "highway"
            "#},
            0..=14,
        )
        .map(|_| ())
        .expect_err("unknown property");
        insta::assert_snapshot!(err, @"layer `roads`: `highway` references unknown property `highway`");
    }

    #[test]
    #[expect(clippy::too_many_lines)]
    fn rules_lower_into_the_layer() {
        let scan = lower(
            indoc! {r#"
                schema: public
                table: places
                srid: 4326
                geometry_column: geom
                properties:
                  class: text
                  name: text
                  rank: int4
                  population: int8
                layers:
                  places:
                    attributes:
                      kind: class
                    rules:
                      - where: "class == 'city'"
                        minzoom: 2
                        maxzoom: "population > 1000000 ? 14 : 10"
                        simplify: 2
                        attributes:
                          kind: { value: city }
                          label: { expr: name, minzoom: 6 }
                      - where: "rank > 3"
                        min_size: 4
                      - minzoom: "rank"
            "#},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        let sql = scan_sql(&scan.info, &scan.table.columns, None).expect("valid SQL");
        insta::assert_snapshot!(sql.sql, @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))), "class", "name", "population", "rank" FROM "public"."places" WHERE "geom" IS NOT NULL"#);
        insta::assert_debug_snapshot!(scan.table.layers[0].rules, @r#"
        [
            RuleDef {
                when: Some(
                    "class == 'city'",
                ),
                minzoom: Some(
                    Fixed(
                        2,
                    ),
                ),
                maxzoom: Some(
                    Expr(
                        "population > 1000000 ? 14 : 10",
                    ),
                ),
                simplify: Some(
                    PixelThreshold {
                        below_max_zoom: 2.0,
                        at_max_zoom: 2.0,
                    },
                ),
                min_size: None,
                attributes: [
                    ComputedAttr {
                        name: "kind",
                        value: Literal(
                            Str(
                                "city",
                            ),
                        ),
                        zooms: 0..=27,
                    },
                    ComputedAttr {
                        name: "label",
                        value: Expr(
                            "name",
                        ),
                        zooms: 6..=27,
                    },
                ],
            },
            RuleDef {
                when: Some(
                    "rank > 3",
                ),
                minzoom: None,
                maxzoom: None,
                simplify: None,
                min_size: Some(
                    PixelThreshold {
                        below_max_zoom: 4.0,
                        at_max_zoom: 4.0,
                    },
                ),
                attributes: [],
            },
            RuleDef {
                when: None,
                minzoom: Some(
                    Expr(
                        "rank",
                    ),
                ),
                maxzoom: None,
                simplify: None,
                min_size: None,
                attributes: [],
            },
        ]
        "#);
    }

    #[test]
    fn sort_by_lowers_into_the_layer_and_reads_its_columns() {
        let scan = lower(
            indoc! {r#"
                schema: public
                table: roads
                srid: 4326
                geometry_column: geom
                properties:
                  class: text
                  name: text
                  rank: int4
                layers:
                  roads:
                    attributes: [name]
                    sort_by: [class, { expr: "rank * 2", desc: true }]
            "#},
            0..=14,
        )
        .expect("lowers")
        .expect("has a layer");
        insta::assert_debug_snapshot!((&scan.table.columns, &scan.table.layers[0].sort_by), @r#"
        (
            [
                "class",
                "name",
                "rank",
            ],
            [
                SortDef {
                    expr: "class",
                    descending: false,
                },
                SortDef {
                    expr: "rank * 2",
                    descending: true,
                },
            ],
        )
        "#);
    }

    #[test]
    fn unsupported_settings_are_rejected() {
        let layers = [
            "roads: { geometry: centroid }",
            "roads: { geometry: label_point }",
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
        layer `roads`: `tile: merge_lines` is not supported by martin generate yet
        layer `roads`: `tile: limit` is not supported by martin generate yet
        ");
    }
}
