//! Lowers table sources and their `layers` into the tables of a [`martin_tilegen::plan::Plan`].

use std::ops::RangeInclusive;

use martin_tilegen::plan::{AttributesDef, GeometryType, IdDef, LayerDef, TableDef};
use martin_tilegen::{LayerGrid, PixelThreshold};
use tracing::warn;

use super::postgres::ScanTable;
use super::{GenerateError, GenerateResult};
use crate::config::file::postgres::TableInfo;
use crate::config::file::tiling::{
    Attributes, IdPolicy, Layer, OutputGeometry, Pixels, PropertySelector, ZoomSetting,
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
    let selected: Vec<(&str, bool)> = properties
        .into_iter()
        .filter(|&(column, jsonb)| {
            layers.iter().any(|layer| match &layer.attributes {
                AttributesDef::All => true,
                AttributesDef::None | AttributesDef::Computed { .. } => false,
                AttributesDef::Columns(keys) if jsonb => keys
                    .iter()
                    .any(|key| !static_columns.contains(&key.as_str())),
                AttributesDef::Columns(keys) => keys.iter().any(|key| key == column),
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
        filter: None,
        minzoom_expr: None,
        maxzoom_expr: None,
        id: match layer.id {
            IdPolicy::Keep | IdPolicy::Expr(_) => IdDef::Keep,
            IdPolicy::Drop => IdDef::Drop,
        },
        attributes: match &layer.attributes {
            Attributes::None => AttributesDef::None,
            Attributes::Properties(selectors) => {
                AttributesDef::Columns(expand(selectors.as_slice(), static_columns))
            }
            Attributes::AllProperties | Attributes::Columns(_) => AttributesDef::All,
        },
    }))
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
    let is_expr = |zoom: Option<&ZoomSetting>| zoom.is_some_and(|z| z.fixed().is_none());
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
    if layer.r#where.is_some() {
        Some("`where`".to_owned())
    } else if let Some(geometry) = geometry {
        Some(format!("`geometry: {geometry}`"))
    } else if is_expr(layer.minzoom.as_ref()) {
        Some("a `minzoom` expression".to_owned())
    } else if is_expr(layer.maxzoom.as_ref()) {
        Some("a `maxzoom` expression".to_owned())
    } else if matches!(layer.attributes, Attributes::Columns(_)) {
        Some("a map of `attributes`".to_owned())
    } else if layer.rules.is_some() {
        Some("`rules`".to_owned())
    } else if layer.sort_by.is_some() {
        Some("`sort_by`".to_owned())
    } else if matches!(layer.id, IdPolicy::Expr(_)) {
        Some("an `id` expression".to_owned())
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
                    id: Keep,
                    attributes: All,
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
                    id: Keep,
                    attributes: All,
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
                    id: Drop,
                    attributes: All,
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
                    id: Keep,
                    attributes: None,
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
                    id: Keep,
                    attributes: All,
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
    fn unsupported_settings_are_rejected() {
        let layers = [
            r#"roads: { where: "class == 'primary'" }"#,
            "roads: { geometry: centroid }",
            "roads: { geometry: label_point }",
            "roads: { minzoom: 'rank > 3 ? 10 : 4' }",
            "roads: { maxzoom: 'rank > 3 ? 14 : 8' }",
            "roads: { attributes: { kind: class } }",
            r#"roads: { attributes: { "name:*": "name:*" } }"#,
            "roads: { rules: [ { where: \"class == 'primary'\", minzoom: 4 } ] }",
            "roads: { sort_by: rank }",
            "roads: { id: { expr: 'gid * 10' } }",
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
        layer `roads`: `where` is not supported by martin generate yet
        layer `roads`: `geometry: centroid` is not supported by martin generate yet
        layer `roads`: `geometry: label_point` is not supported by martin generate yet
        layer `roads`: a `minzoom` expression is not supported by martin generate yet
        layer `roads`: a `maxzoom` expression is not supported by martin generate yet
        layer `roads`: a map of `attributes` is not supported by martin generate yet
        layer `roads`: a map of `attributes` is not supported by martin generate yet
        layer `roads`: `rules` is not supported by martin generate yet
        layer `roads`: `sort_by` is not supported by martin generate yet
        layer `roads`: an `id` expression is not supported by martin generate yet
        layer `roads`: `tile: merge_lines` is not supported by martin generate yet
        layer `roads`: `tile: limit` is not supported by martin generate yet
        ");
    }
}
