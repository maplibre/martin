//! What to generate: source tables, and the output layers each table feeds.

use std::collections::HashSet;
use std::ops::{Range, RangeInclusive};

use crate::props::KeyId;
use crate::{
    FeatureOrder, LayerGrid, LayerInfo, MAX_ZOOM, PixelThreshold, RenderLayer, TileGenError,
    TileGenResult,
};

/// A table as its source scans it; its position in [`Plan::new`] is the [`table`] of its batches.
///
/// [`table`]: crate::source::FeatureBatch::table
#[derive(Clone, Debug)]
pub struct TableDef {
    /// Property keys known up front, in column order: column `i` is [`KeyId::from(i)`](KeyId::from).
    pub columns: Vec<String>,
    /// Whether features may carry keys beyond `columns`, e.g. from a JSON column.
    pub dynamic_props: bool,
    pub layers: Vec<LayerDef>,
}

#[derive(Clone, Debug)]
pub struct LayerDef {
    pub name: String,
    pub zooms: RangeInclusive<u8>,
    pub grid: LayerGrid,
    /// `false` keeps whole features in every tile they touch, like `ST_AsMVTGeom(..., clip_geom => false)`.
    pub clip: bool,
    /// RDP tolerance.
    pub simplify: PixelThreshold,
    /// Lines and polygons whose bounding box is smaller than this are dropped.
    pub min_size: PixelThreshold,
    /// WGS84 `[min_lon, min_lat, max_lon, max_lat]`: only tiles intersecting it are generated.
    pub bounds: Option<[f64; 4]>,
    pub order: FeatureOrder,
    pub id: IdDef,
    pub attributes: AttributesDef,
}

impl LayerDef {
    /// Clipped, unbounded, in source order, keeping ids and all attributes, with
    /// [`PixelThreshold::PLANETILER_SIMPLIFY`] and [`PixelThreshold::PLANETILER_MIN_SIZE`].
    #[must_use]
    pub fn new(name: impl Into<String>, zooms: RangeInclusive<u8>, grid: LayerGrid) -> Self {
        Self {
            name: name.into(),
            zooms,
            grid,
            clip: true,
            simplify: PixelThreshold::PLANETILER_SIMPLIFY,
            min_size: PixelThreshold::PLANETILER_MIN_SIZE,
            bounds: None,
            order: FeatureOrder::Source,
            id: IdDef::Keep,
            attributes: AttributesDef::All,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IdDef {
    #[default]
    Keep,
    Drop,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AttributesDef {
    /// Every column and dynamic key.
    #[default]
    All,
    None,
    /// These keys, in this column order; a key that is not a table column needs `dynamic_props`.
    Columns(Vec<String>),
}

/// Validated [`TableDef`]s, with output layers numbered table by table, then in declaration order: that
/// number is the layer byte of the sort key, and so the layer's order inside each tile.
#[derive(Debug)]
pub struct Plan {
    pub(crate) tables: Vec<PlannedTable>,
    pub(crate) layers: Vec<PlannedLayer>,
}

#[derive(Debug)]
pub(crate) struct PlannedTable {
    pub(crate) columns: Vec<String>,
    pub(crate) layers: Range<usize>,
}

#[derive(Debug)]
pub(crate) struct PlannedLayer {
    pub(crate) render: RenderLayer,
    pub(crate) info: LayerInfo,
    pub(crate) zooms: RangeInclusive<u8>,
    pub(crate) simplify: PixelThreshold,
    pub(crate) min_size: PixelThreshold,
    pub(crate) id: IdDef,
    pub(crate) attributes: AttributesDef,
    /// The output layer's known keys.
    pub(crate) keys: Vec<String>,
    /// Output key by table column.
    pub(crate) copy: Vec<Option<KeyId>>,
}

impl Plan {
    pub fn new(tables: Vec<TableDef>) -> TileGenResult<Self> {
        let total = tables.iter().map(|t| t.layers.len()).sum();
        if total > 256 {
            return Err(TileGenError::TooManyLayers(total));
        }
        if u16::try_from(tables.len()).is_err() {
            return Err(TileGenError::TooManyTables(tables.len()));
        }
        let mut names = HashSet::new();
        let mut planned_tables = Vec::with_capacity(tables.len());
        let mut layers = Vec::with_capacity(total);
        for table in tables {
            let start = layers.len();
            for def in table.layers {
                if !names.insert(def.name.clone()) {
                    return Err(TileGenError::DuplicateLayer(def.name));
                }
                if *def.zooms.end() > MAX_ZOOM {
                    return Err(TileGenError::ZoomTooHigh {
                        layer: def.name,
                        zoom: *def.zooms.end(),
                    });
                }
                let index = u8::try_from(layers.len()).expect("at most 256 layers");
                layers.push(plan_layer(index, def, &table.columns, table.dynamic_props)?);
            }
            planned_tables.push(PlannedTable {
                columns: table.columns,
                layers: start..layers.len(),
            });
        }
        Ok(Self {
            tables: planned_tables,
            layers,
        })
    }
}

fn plan_layer(
    index: u8,
    def: LayerDef,
    columns: &[String],
    dynamic_props: bool,
) -> TileGenResult<PlannedLayer> {
    let keys = match &def.attributes {
        AttributesDef::All => columns.to_vec(),
        AttributesDef::None => Vec::new(),
        AttributesDef::Columns(keys) => {
            let mut seen = HashSet::new();
            for key in keys {
                if !seen.insert(key) {
                    return Err(TileGenError::DuplicateAttribute {
                        layer: def.name,
                        key: key.clone(),
                    });
                }
                if !dynamic_props && !columns.contains(key) {
                    return Err(TileGenError::UnknownColumn {
                        layer: def.name,
                        column: key.clone(),
                    });
                }
            }
            keys.clone()
        }
    };
    let copy = columns
        .iter()
        .map(|column| {
            keys.iter()
                .position(|key| key == column)
                .map(|pos| KeyId::from(u32::try_from(pos).expect("fewer than 2^32 keys")))
        })
        .collect();
    let mut render = RenderLayer::new(index, def.zooms.clone(), def.grid)?;
    render.clip = def.clip;
    render.bounds = def.bounds.map(crate::render::unit_bounds);
    Ok(PlannedLayer {
        render,
        info: LayerInfo {
            name: def.name,
            grid: def.grid,
            order: def.order,
        },
        zooms: def.zooms,
        simplify: def.simplify,
        min_size: def.min_size,
        id: def.id,
        attributes: def.attributes,
        keys,
        copy,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: LayerGrid = LayerGrid {
        extent: 4096,
        buffer: 64,
    };

    #[test]
    fn numbers_layers_table_by_table() {
        let plan = Plan::new(vec![
            TableDef {
                columns: vec!["name".to_owned()],
                dynamic_props: false,
                layers: vec![
                    LayerDef::new("roads", 0..=14, GRID),
                    LayerDef::new("road_labels", 10..=14, GRID),
                ],
            },
            TableDef {
                columns: vec![],
                dynamic_props: false,
                layers: vec![LayerDef::new("water", 0..=14, GRID)],
            },
        ])
        .unwrap();
        let layers: Vec<_> = plan
            .layers
            .iter()
            .map(|l| (l.render.index, l.info.name.as_str()))
            .collect();
        assert_eq!(layers, [(0, "roads"), (1, "road_labels"), (2, "water")]);
        let tables: Vec<_> = plan.tables.iter().map(|t| t.layers.clone()).collect();
        assert_eq!(tables, [0..2, 2..3]);
    }

    #[test]
    fn copies_attribute_columns_in_their_order() {
        let plan = Plan::new(vec![TableDef {
            columns: vec!["name".to_owned(), "class".to_owned(), "lanes".to_owned()],
            dynamic_props: false,
            layers: vec![
                LayerDef::new("all", 0..=14, GRID),
                LayerDef {
                    attributes: AttributesDef::Columns(vec!["lanes".to_owned(), "name".to_owned()]),
                    ..LayerDef::new("subset", 0..=14, GRID)
                },
                LayerDef {
                    attributes: AttributesDef::None,
                    ..LayerDef::new("none", 0..=14, GRID)
                },
            ],
        }])
        .unwrap();
        let copies: Vec<_> = plan.layers.iter().map(|l| l.copy.clone()).collect();
        assert_eq!(
            copies,
            [
                vec![Some(KeyId(0)), Some(KeyId(1)), Some(KeyId(2))],
                vec![Some(KeyId(1)), None, Some(KeyId(0))],
                vec![None, None, None],
            ]
        );
        assert_eq!(plan.layers[1].keys, ["lanes", "name"]);
    }

    #[test]
    fn rejects_a_repeated_layer_name() {
        let err = Plan::new(vec![
            TableDef {
                columns: vec![],
                dynamic_props: false,
                layers: vec![LayerDef::new("roads", 0..=14, GRID)],
            },
            TableDef {
                columns: vec![],
                dynamic_props: false,
                layers: vec![LayerDef::new("roads", 0..=14, GRID)],
            },
        ])
        .unwrap_err();
        assert_eq!(err.to_string(), "layer name `roads` is used more than once");
    }

    #[test]
    fn rejects_more_than_256_layers() {
        let err = Plan::new(vec![TableDef {
            columns: vec![],
            dynamic_props: false,
            layers: (0..257)
                .map(|i| LayerDef::new(format!("l{i}"), 0..=0, GRID))
                .collect(),
        }])
        .unwrap_err();
        assert!(matches!(err, TileGenError::TooManyLayers(257)), "{err}");
    }

    #[test]
    fn rejects_zooms_above_the_max() {
        let err = Plan::new(vec![TableDef {
            columns: vec![],
            dynamic_props: false,
            layers: vec![LayerDef::new(
                "deep",
                0..=28,
                LayerGrid {
                    extent: 1,
                    buffer: 0,
                },
            )],
        }])
        .unwrap_err();
        assert_eq!(err.to_string(), "layer `deep`: zoom 28 is above 27");
    }

    #[test]
    fn rejects_unknown_attribute_columns_without_dynamic_props() {
        let err = Plan::new(vec![TableDef {
            columns: vec!["name".to_owned()],
            dynamic_props: false,
            layers: vec![LayerDef {
                attributes: AttributesDef::Columns(vec!["kind".to_owned()]),
                ..LayerDef::new("roads", 0..=14, GRID)
            }],
        }])
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "layer `roads`: attribute `kind` is not a table column"
        );
        let plan = Plan::new(vec![TableDef {
            columns: vec!["name".to_owned()],
            dynamic_props: true,
            layers: vec![LayerDef {
                attributes: AttributesDef::Columns(vec!["kind".to_owned()]),
                ..LayerDef::new("roads", 0..=14, GRID)
            }],
        }])
        .unwrap();
        assert_eq!(plan.layers[0].copy, [None]);
    }
}
