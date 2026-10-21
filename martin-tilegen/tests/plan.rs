//! What `Plan::new` accepts and rejects, which columns it scans, and what each layer makes of a feature.
#![expect(clippy::unwrap_used)]

use geo_types::{Coord, LineString, Polygon};
use martin_tilegen::plan::{
    AttributesDef, ColumnDef, ComputedAttr, GeometryType, IdDef, LayerDef, Plan, PlanError, RowFilter, RuleDef,
    SortDef, TableDef, ValueDef, ValueKind, ZoomDef,
};
use martin_tilegen::props::{KeyId, Prop};
use martin_tilegen::source::{Geometry, SourceFeature};
use martin_tilegen::{LayerGrid, PixelThreshold};

const GRID: LayerGrid = LayerGrid {
    extent: 4096,
    buffer: 64,
};

fn evaluate(plan: &Plan, features: &[SourceFeature]) -> String {
    let mut evaluator = plan.evaluator();
    let mut lines = Vec::new();
    for feature in features {
        let geom = feature.geometry.feature_geom().expect("unprojected geometry");
        evaluator
            .evaluate(0, feature, geom, |evaluated| {
                lines.push(format!("{evaluated:?}"));
                Ok(())
            })
            .unwrap();
    }
    let errors = plan.expr_errors(&evaluator.finish());
    lines.extend(errors.iter().map(|errors| format!("{errors:?}")));
    lines.join("\n")
}

#[test]
fn rejects_a_repeated_layer_name() {
    let err = Plan::new(vec![
        TableDef {
            columns: vec![],
            layers: vec![LayerDef::new("roads", 0..=14, GRID)],
        },
        TableDef {
            columns: vec![],
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
        layers: (0..257).map(|i| LayerDef::new(format!("l{i}"), 0..=0, GRID)).collect(),
    }])
    .unwrap_err();
    assert!(matches!(err, PlanError::TooManyLayers(257)), "{err}");
}

#[test]
fn accepts_256_layers() {
    Plan::new(vec![TableDef {
        columns: vec![],
        layers: (0..256).map(|i| LayerDef::new(format!("l{i}"), 0..=0, GRID)).collect(),
    }])
    .unwrap();
}

#[test]
fn rejects_zooms_above_the_max() {
    let err = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef::new("deep", 0..=28, GRID)],
    }])
    .unwrap_err();
    assert_eq!(err.to_string(), "layer `deep`: zoom 28 is above 27");
}

#[test]
fn rejects_an_attribute_that_is_not_a_column() {
    let err = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("name".to_owned(), None)],
        layers: vec![LayerDef {
            attributes: AttributesDef::Columns(vec!["kind".to_owned()]),
            ..LayerDef::new("roads", 0..=14, GRID)
        }],
    }])
    .unwrap_err();
    assert_eq!(err.to_string(), "layer `roads`: attribute `kind` is not a table column");
}

#[test]
fn rejects_a_repeated_attribute_column() {
    let err = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("name".to_owned(), None)],
        layers: vec![LayerDef {
            attributes: AttributesDef::Columns(vec!["name".to_owned(), "name".to_owned()]),
            ..LayerDef::new("roads", 0..=14, GRID)
        }],
    }])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "layer `roads`: attribute `name` is listed more than once"
    );
}

#[test]
fn rejects_a_repeated_computed_attribute() {
    let attr = ComputedAttr {
        name: "big".to_owned(),
        value: ValueDef::Literal(Prop::Bool(true)),
        zooms: 0..=30,
    };
    let err = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef {
            attributes: AttributesDef::Computed {
                attributes: vec![attr.clone(), attr],
                prefixes: vec![],
            },
            ..LayerDef::new("roads", 0..=14, GRID)
        }],
    }])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "layer `roads`: attribute `big` is listed more than once"
    );
}

#[test]
fn rejects_a_rule_repeating_an_attribute() {
    let attr = ComputedAttr {
        name: "big".to_owned(),
        value: ValueDef::Literal(Prop::Bool(true)),
        zooms: 0..=30,
    };
    let err = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef {
            rules: vec![RuleDef {
                when: Some("true".to_owned()),
                attributes: vec![attr.clone(), attr],
                ..RuleDef::default()
            }],
            ..LayerDef::new("roads", 0..=14, GRID)
        }],
    }])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "layer `roads`: attribute `big` is listed more than once"
    );
}

#[test]
fn rejects_an_expression_naming_an_unknown_property() {
    let err = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![LayerDef {
            filter: Some("kind == 'primary'".to_owned()),
            ..LayerDef::new("roads", 0..=14, GRID)
        }],
    }])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "layer `roads`: `kind == 'primary'` references unknown property `kind`"
    );
}

#[test]
fn all_attributes_scan_every_column() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("class".to_owned(), None),
            ColumnDef::Keys("tags".to_owned()),
            ColumnDef::Value("name".to_owned(), None),
        ],
        layers: vec![LayerDef::new("roads", 0..=14, GRID)],
    }])
    .unwrap();
    insta::assert_debug_snapshot!(plan.columns(0), @r#"
    [
        "class",
        "tags",
        "name",
    ]
    "#);
}

#[test]
fn only_the_columns_some_layer_copies_are_scanned() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("class".to_owned(), None),
            ColumnDef::Value("name".to_owned(), None),
            ColumnDef::Value("rank".to_owned(), None),
            ColumnDef::Keys("tags".to_owned()),
        ],
        layers: vec![
            LayerDef {
                attributes: AttributesDef::Columns(vec!["rank".to_owned()]),
                ..LayerDef::new("roads", 0..=14, GRID)
            },
            LayerDef {
                attributes: AttributesDef::Computed {
                    attributes: vec![],
                    prefixes: vec!["na".to_owned()],
                },
                ..LayerDef::new("road_labels", 0..=14, GRID)
            },
        ],
    }])
    .unwrap();
    insta::assert_debug_snapshot!(plan.columns(0), @r#"
    [
        "name",
        "rank",
    ]
    "#);
}

#[test]
fn layers_without_attributes_scan_no_columns() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("name".to_owned(), None),
            ColumnDef::Keys("tags".to_owned()),
        ],
        layers: vec![
            LayerDef {
                attributes: AttributesDef::None,
                ..LayerDef::new("roads", 0..=14, GRID)
            },
            LayerDef {
                attributes: AttributesDef::None,
                ..LayerDef::new("road_shapes", 0..=14, GRID)
            },
        ],
    }])
    .unwrap();
    insta::assert_debug_snapshot!(plan.columns(0), @"[]");
}

#[test]
fn expressions_keep_the_columns_they_read() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("class".to_owned(), None),
            ColumnDef::Value("gid".to_owned(), None),
            ColumnDef::Value("lanes".to_owned(), None),
            ColumnDef::Value("name".to_owned(), None),
            ColumnDef::Value("rank".to_owned(), None),
            ColumnDef::Value("surface".to_owned(), None),
            ColumnDef::Keys("tags".to_owned()),
        ],
        layers: vec![LayerDef {
            filter: Some("class == 'primary'".to_owned()),
            minzoom_expr: Some("rank".to_owned()),
            id: IdDef::Expr("gid".to_owned()),
            attributes: AttributesDef::Computed {
                attributes: vec![
                    ComputedAttr {
                        name: "label".to_owned(),
                        value: ValueDef::Expr("name".to_owned()),
                        zooms: 10..=14,
                    },
                    ComputedAttr {
                        name: "wide".to_owned(),
                        value: ValueDef::Expr("lanes > 2".to_owned()),
                        zooms: 0..=14,
                    },
                ],
                prefixes: vec![],
            },
            ..LayerDef::new("roads", 0..=14, GRID)
        }],
    }])
    .unwrap();
    insta::assert_debug_snapshot!(plan.columns(0), @r#"
    [
        "class",
        "gid",
        "lanes",
        "name",
        "rank",
    ]
    "#);
}

#[test]
fn an_expression_reading_a_key_that_is_not_a_column_scans_the_keys_columns() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("name".to_owned(), None),
            ColumnDef::Keys("tags".to_owned()),
            ColumnDef::Keys("other_tags".to_owned()),
        ],
        layers: vec![LayerDef {
            filter: Some("highway == 'primary'".to_owned()),
            attributes: AttributesDef::None,
            ..LayerDef::new("roads", 0..=14, GRID)
        }],
    }])
    .unwrap();
    insta::assert_debug_snapshot!(plan.columns(0), @r#"
    [
        "tags",
        "other_tags",
    ]
    "#);
}

#[test]
fn an_attribute_that_is_not_a_column_scans_the_keys_columns() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("name".to_owned(), None),
            ColumnDef::Keys("tags".to_owned()),
        ],
        layers: vec![LayerDef {
            attributes: AttributesDef::Columns(vec!["highway".to_owned()]),
            ..LayerDef::new("roads", 0..=14, GRID)
        }],
    }])
    .unwrap();
    insta::assert_debug_snapshot!(plan.columns(0), @r#"
    [
        "tags",
    ]
    "#);
}

#[test]
fn an_expression_indexing_the_feature_by_a_computed_key_scans_every_column() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("lang".to_owned(), None),
            ColumnDef::Value("name:de".to_owned(), None),
            ColumnDef::Value("name:en".to_owned(), None),
            ColumnDef::Keys("tags".to_owned()),
        ],
        layers: vec![LayerDef {
            attributes: AttributesDef::Computed {
                attributes: vec![ComputedAttr {
                    name: "label".to_owned(),
                    value: ValueDef::Expr("feature['name:' + lang]".to_owned()),
                    zooms: 0..=14,
                }],
                prefixes: vec![],
            },
            ..LayerDef::new("places", 0..=14, GRID)
        }],
    }])
    .unwrap();
    insta::assert_debug_snapshot!(plan.columns(0), @r#"
    [
        "lang",
        "name:de",
        "name:en",
        "tags",
    ]
    "#);
}

#[test]
fn attribute_prefixes_do_not_match_keys_columns() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("tag_count".to_owned(), None),
            ColumnDef::Keys("tags".to_owned()),
        ],
        layers: vec![LayerDef {
            attributes: AttributesDef::Computed {
                attributes: vec![],
                prefixes: vec!["tag".to_owned()],
            },
            ..LayerDef::new("roads", 0..=14, GRID)
        }],
    }])
    .unwrap();
    insta::assert_debug_snapshot!(plan.columns(0), @r#"
    [
        "tag_count",
    ]
    "#);
}

#[test]
fn each_table_scans_the_columns_of_its_own_layers() {
    let plan = Plan::new(vec![
        TableDef {
            columns: vec![
                ColumnDef::Value("class".to_owned(), None),
                ColumnDef::Value("name".to_owned(), None),
            ],
            layers: vec![LayerDef {
                attributes: AttributesDef::Columns(vec!["class".to_owned()]),
                ..LayerDef::new("roads", 0..=14, GRID)
            }],
        },
        TableDef {
            columns: vec![
                ColumnDef::Value("class".to_owned(), None),
                ColumnDef::Value("name".to_owned(), None),
            ],
            layers: vec![LayerDef {
                attributes: AttributesDef::Columns(vec!["name".to_owned()]),
                ..LayerDef::new("places", 0..=14, GRID)
            }],
        },
    ])
    .unwrap();
    insta::assert_debug_snapshot!((plan.columns(0), plan.columns(1)), @r#"
    (
        [
            "class",
        ],
        [
            "name",
        ],
    )
    "#);
}

#[test]
fn rule_expressions_keep_the_columns_they_read() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("class".to_owned(), None),
            ColumnDef::Value("name".to_owned(), None),
            ColumnDef::Value("population".to_owned(), None),
            ColumnDef::Value("rank".to_owned(), None),
            ColumnDef::Value("surface".to_owned(), None),
        ],
        layers: vec![LayerDef {
            attributes: AttributesDef::None,
            rules: vec![
                RuleDef {
                    when: Some("class == 'city'".to_owned()),
                    maxzoom: Some(ZoomDef::Expr("population > 1000000 ? 14 : 10".to_owned())),
                    attributes: vec![ComputedAttr {
                        name: "label".to_owned(),
                        value: ValueDef::Expr("name".to_owned()),
                        zooms: 6..=30,
                    }],
                    ..RuleDef::default()
                },
                RuleDef {
                    minzoom: Some(ZoomDef::Expr("rank".to_owned())),
                    ..RuleDef::default()
                },
            ],
            ..LayerDef::new("places", 0..=14, GRID)
        }],
    }])
    .unwrap();
    insta::assert_debug_snapshot!(plan.columns(0), @r#"
    [
        "class",
        "name",
        "population",
        "rank",
    ]
    "#);
}

#[test]
fn sort_by_keeps_the_columns_it_reads() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("class".to_owned(), None),
            ColumnDef::Value("name".to_owned(), None),
            ColumnDef::Value("rank".to_owned(), None),
        ],
        layers: vec![LayerDef {
            attributes: AttributesDef::None,
            sort_by: vec![
                SortDef {
                    expr: "class".to_owned(),
                    descending: false,
                },
                SortDef {
                    expr: "rank * 2".to_owned(),
                    descending: true,
                },
            ],
            ..LayerDef::new("roads", 0..=14, GRID)
        }],
    }])
    .unwrap();
    insta::assert_debug_snapshot!(plan.columns(0), @r#"
    [
        "class",
        "rank",
    ]
    "#);
}

#[test]
fn a_layer_takes_only_features_of_its_geometry_type() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![
            LayerDef {
                geometry: Some(GeometryType::Point),
                ..LayerDef::new("points", 0..=0, GRID)
            },
            LayerDef {
                geometry: Some(GeometryType::Line),
                ..LayerDef::new("lines", 0..=0, GRID)
            },
            LayerDef {
                geometry: Some(GeometryType::Polygon),
                ..LayerDef::new("polygons", 0..=0, GRID)
            },
            LayerDef::new("everything", 0..=0, GRID),
        ],
    }])
    .unwrap();
    let features = [
        SourceFeature {
            id: Some(1),
            geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
            props: vec![],
        },
        SourceFeature {
            id: Some(2),
            geometry: Geometry::Lines(vec![LineString::from(vec![(10.0, 10.0), (30.0, 20.0)])]),
            props: vec![],
        },
        SourceFeature {
            id: Some(3),
            geometry: Geometry::Polygons(vec![Polygon::new(
                LineString::from(vec![(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 10.0)]),
                vec![],
            )]),
            props: vec![],
        },
    ];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"
    LayerFeature { layer: "points", id: Some(1), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "everything", id: Some(1), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "lines", id: Some(2), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "everything", id: Some(2), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "polygons", id: Some(3), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "everything", id: Some(3), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    "#);
}

#[test]
fn a_where_filter_picks_the_features_of_each_layer() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("class".to_owned(), None),
            ColumnDef::Value("rank".to_owned(), None),
        ],
        layers: vec![
            LayerDef {
                filter: Some("class == 'primary'".to_owned()),
                attributes: AttributesDef::None,
                ..LayerDef::new("primary", 0..=0, GRID)
            },
            LayerDef {
                filter: Some("rank >= 4".to_owned()),
                attributes: AttributesDef::None,
                ..LayerDef::new("ranked", 0..=0, GRID)
            },
        ],
    }])
    .unwrap();
    let point = |id, class: &str, rank| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![
            (KeyId::new(0), Prop::Str(class.to_owned())),
            (KeyId::new(1), Prop::I64(rank)),
        ],
    };
    let features = [
        point(1, "primary", 5),
        point(2, "service", 1),
        point(3, "primary", 1),
        point(4, "secondary", 4),
    ];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"
    LayerFeature { layer: "primary", id: Some(1), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "ranked", id: Some(1), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "primary", id: Some(3), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "ranked", id: Some(4), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    "#);
}

#[test]
fn zoom_expressions_set_each_features_zooms() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("rank".to_owned(), None)],
        layers: vec![LayerDef {
            minzoom_expr: Some("rank >= 10 ? 0 : (rank > 0 ? 3 - rank : 20)".to_owned()),
            maxzoom_expr: Some("rank < 0 ? 0 : (rank == 10 ? 1 : null)".to_owned()),
            attributes: AttributesDef::None,
            ..LayerDef::new("places", 1..=4, GRID)
        }],
    }])
    .unwrap();
    let point = |id, rank| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![(KeyId::new(0), Prop::I64(rank))],
    };
    let features = [point(1, 10), point(2, 2), point(3, 0), point(4, -1)];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"
    LayerFeature { layer: "places", id: Some(1), zooms: 1..=1, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "places", id: Some(2), zooms: 1..=4, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    "#);
}

#[test]
fn computed_attributes_appear_at_their_own_zooms() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("name".to_owned(), None),
            ColumnDef::Value("name:en".to_owned(), None),
            ColumnDef::Value("class".to_owned(), None),
            ColumnDef::Value("rank".to_owned(), None),
        ],
        layers: vec![LayerDef {
            attributes: AttributesDef::Computed {
                attributes: vec![
                    ComputedAttr {
                        name: "kind".to_owned(),
                        value: ValueDef::Expr("class".to_owned()),
                        zooms: 0..=30,
                    },
                    ComputedAttr {
                        name: "label".to_owned(),
                        value: ValueDef::Expr("name".to_owned()),
                        zooms: 2..=30,
                    },
                    ComputedAttr {
                        name: "big".to_owned(),
                        value: ValueDef::Expr("rank * 2 > 10".to_owned()),
                        zooms: 0..=2,
                    },
                    ComputedAttr {
                        name: "source".to_owned(),
                        value: ValueDef::Literal(Prop::Str("osm".to_owned())),
                        zooms: 0..=30,
                    },
                    ComputedAttr {
                        name: "missing".to_owned(),
                        value: ValueDef::Expr("null".to_owned()),
                        zooms: 0..=30,
                    },
                ],
                prefixes: vec!["name:".to_owned()],
            },
            ..LayerDef::new("water", 0..=3, GRID)
        }],
    }])
    .unwrap();
    let features = [SourceFeature {
        id: Some(1),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![
            (KeyId::new(0), Prop::Str("Rhein".to_owned())),
            (KeyId::new(1), Prop::Str("Rhine".to_owned())),
            (KeyId::new(2), Prop::Str("river".to_owned())),
            (KeyId::new(3), Prop::I64(7)),
        ],
    }];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"LayerFeature { layer: "water", id: Some(1), zooms: 0..=3, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {"name:en": Str("Rhine"), "kind": Str("river"), "source": Str("osm"), "big": Bool(true)}, bands: [(2, {"name:en": Str("Rhine"), "kind": Str("river"), "source": Str("osm"), "label": Str("Rhein"), "big": Bool(true)}), (3, {"name:en": Str("Rhine"), "kind": Str("river"), "source": Str("osm"), "label": Str("Rhein")})], sort: None }"#);
}

#[test]
fn an_id_expression_sets_the_feature_id() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("gid".to_owned(), None)],
        layers: vec![LayerDef {
            id: IdDef::Expr("gid == 0 ? null : gid * 10".to_owned()),
            attributes: AttributesDef::None,
            ..LayerDef::new("places", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let point = |id, gid| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![(KeyId::new(0), Prop::I64(gid))],
    };
    let features = [point(1, 7), point(2, -3), point(3, 0)];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"
    LayerFeature { layer: "places", id: Some(70), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "places", id: None, zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "places", id: None, zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    ExprErrors { layer: "places", expr: "gid == 0 ? null : gid * 10", errors: 1 }
    "#);
}

#[test]
fn failing_expressions_count_as_null_and_are_counted() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("lanes".to_owned(), None)],
        layers: vec![LayerDef {
            filter: Some("lanes != 1".to_owned()),
            attributes: AttributesDef::Computed {
                attributes: vec![ComputedAttr {
                    name: "width".to_owned(),
                    value: ValueDef::Expr("8 / lanes".to_owned()),
                    zooms: 0..=30,
                }],
                prefixes: vec![],
            },
            ..LayerDef::new("roads", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let point = |id, props| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props,
    };
    let features = [
        point(1, vec![(KeyId::new(0), Prop::I64(4))]),
        point(2, vec![(KeyId::new(0), Prop::I64(0))]),
        point(3, vec![(KeyId::new(0), Prop::Str("x".to_owned()))]),
        point(4, vec![]),
    ];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"
    LayerFeature { layer: "roads", id: Some(1), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {"width": I64(2)}, bands: [], sort: None }
    LayerFeature { layer: "roads", id: Some(2), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "roads", id: Some(3), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "roads", id: Some(4), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    ExprErrors { layer: "roads", expr: "8 / lanes", errors: 3 }
    "#);
}

#[test]
fn expressions_read_keys_the_source_interns_as_it_goes() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Keys("tags".to_owned())],
        layers: vec![LayerDef {
            filter: Some("tag1 == true".to_owned()),
            attributes: AttributesDef::Computed {
                attributes: vec![ComputedAttr {
                    name: "tagged".to_owned(),
                    value: ValueDef::Expr("'tag1' in feature".to_owned()),
                    zooms: 0..=30,
                }],
                prefixes: vec![],
            },
            ..LayerDef::new("places", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let keys = &plan.table_keys()[0];
    let features: Vec<_> = (0..4u64)
        .map(|i| SourceFeature {
            id: Some(i),
            geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
            props: vec![(keys.intern(&format!("tag{}", i % 2)), Prop::Bool(true))],
        })
        .collect();
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"
    LayerFeature { layer: "places", id: Some(1), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {"tagged": Bool(true)}, bands: [], sort: None }
    LayerFeature { layer: "places", id: Some(3), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {"tagged": Bool(true)}, bands: [], sort: None }
    "#);
}

#[test]
fn an_attribute_reads_its_column_after_unused_columns_are_dropped() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("class".to_owned(), None),
            ColumnDef::Value("name".to_owned(), None),
        ],
        layers: vec![LayerDef {
            attributes: AttributesDef::Computed {
                attributes: vec![ComputedAttr {
                    name: "label".to_owned(),
                    value: ValueDef::Expr("name".to_owned()),
                    zooms: 1..=30,
                }],
                prefixes: vec![],
            },
            ..LayerDef::new("water", 0..=1, GRID)
        }],
    }])
    .unwrap();
    assert_eq!(plan.columns(0), ["name"]);
    let features = [SourceFeature {
        id: Some(1),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![(KeyId::new(0), Prop::Str("Rhein".to_owned()))],
    }];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"LayerFeature { layer: "water", id: Some(1), zooms: 0..=1, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [(1, {"label": Str("Rhein")})], sort: None }"#);
}

#[test]
fn a_json_key_named_like_its_column_is_kept() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("name".to_owned(), None),
            ColumnDef::Keys("tags".to_owned()),
        ],
        layers: vec![LayerDef::new("places", 0..=0, GRID)],
    }])
    .unwrap();
    assert_eq!(plan.columns(0), ["name", "tags"]);
    let features = [SourceFeature {
        id: Some(1),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![
            (KeyId::new(0), Prop::Str("Bonn".to_owned())),
            (KeyId::new(1), Prop::Str("city".to_owned())),
        ],
    }];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"LayerFeature { layer: "places", id: Some(1), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {"name": Str("Bonn"), "tags": Str("city")}, bands: [], sort: None }"#);
}

#[test]
fn the_first_matching_rule_wins() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("rank".to_owned(), None)],
        layers: vec![LayerDef {
            attributes: AttributesDef::None,
            rules: vec![
                RuleDef {
                    when: Some("rank >= 10".to_owned()),
                    minzoom: Some(ZoomDef::Fixed(0)),
                    ..RuleDef::default()
                },
                RuleDef {
                    when: Some("rank >= 5".to_owned()),
                    minzoom: Some(ZoomDef::Fixed(1)),
                    ..RuleDef::default()
                },
                RuleDef {
                    when: Some("rank >= 1".to_owned()),
                    minzoom: Some(ZoomDef::Fixed(2)),
                    ..RuleDef::default()
                },
                RuleDef {
                    when: None,
                    minzoom: Some(ZoomDef::Fixed(3)),
                    ..RuleDef::default()
                },
            ],
            ..LayerDef::new("places", 0..=3, GRID)
        }],
    }])
    .unwrap();
    let point = |id, rank| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![(KeyId::new(0), Prop::I64(rank))],
    };
    let features = [point(1, 12), point(2, 7), point(3, 3), point(4, 0)];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"
    LayerFeature { layer: "places", id: Some(1), zooms: 0..=3, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "places", id: Some(2), zooms: 1..=3, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "places", id: Some(3), zooms: 2..=3, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "places", id: Some(4), zooms: 3..=3, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    "#);
}

#[test]
fn a_feature_matching_no_rule_keeps_the_layer_settings() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("rank".to_owned(), None)],
        layers: vec![LayerDef {
            minzoom_expr: Some("2".to_owned()),
            attributes: AttributesDef::Computed {
                attributes: vec![ComputedAttr {
                    name: "kind".to_owned(),
                    value: ValueDef::Literal(Prop::Str("layer".to_owned())),
                    zooms: 0..=30,
                }],
                prefixes: vec![],
            },
            rules: vec![RuleDef {
                when: Some("rank > 5".to_owned()),
                minzoom: Some(ZoomDef::Fixed(0)),
                attributes: vec![ComputedAttr {
                    name: "kind".to_owned(),
                    value: ValueDef::Literal(Prop::Str("rule".to_owned())),
                    zooms: 0..=30,
                }],
                ..RuleDef::default()
            }],
            ..LayerDef::new("places", 0..=2, GRID)
        }],
    }])
    .unwrap();
    let point = |id, rank| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![(KeyId::new(0), Prop::I64(rank))],
    };
    let features = [point(1, 9), point(2, 1)];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"
    LayerFeature { layer: "places", id: Some(1), zooms: 0..=2, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {"kind": Str("rule")}, bands: [], sort: None }
    LayerFeature { layer: "places", id: Some(2), zooms: 2..=2, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {"kind": Str("layer")}, bands: [], sort: None }
    "#);
}

#[test]
fn rule_zooms_are_fixed_or_per_feature() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("rank".to_owned(), None)],
        layers: vec![LayerDef {
            maxzoom_expr: Some("3".to_owned()),
            attributes: AttributesDef::None,
            rules: vec![
                RuleDef {
                    when: Some("rank >= 10".to_owned()),
                    minzoom: Some(ZoomDef::Fixed(0)),
                    maxzoom: Some(ZoomDef::Fixed(2)),
                    ..RuleDef::default()
                },
                RuleDef {
                    when: Some("rank > 0".to_owned()),
                    minzoom: Some(ZoomDef::Expr("rank".to_owned())),
                    maxzoom: Some(ZoomDef::Expr("rank > 2 ? null : 2".to_owned())),
                    ..RuleDef::default()
                },
            ],
            ..LayerDef::new("places", 1..=4, GRID)
        }],
    }])
    .unwrap();
    let point = |id, rank| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![(KeyId::new(0), Prop::I64(rank))],
    };
    let features = [point(1, 12), point(2, 2), point(3, 3), point(4, 0)];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"
    LayerFeature { layer: "places", id: Some(1), zooms: 1..=2, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "places", id: Some(2), zooms: 2..=2, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "places", id: Some(3), zooms: 3..=3, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "places", id: Some(4), zooms: 1..=3, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    "#);
}

#[test]
fn a_rule_sets_its_own_simplify() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![LayerDef {
            simplify: PixelThreshold::ZERO,
            min_size: PixelThreshold::ZERO,
            attributes: AttributesDef::None,
            rules: vec![RuleDef {
                when: Some("class == 'smooth'".to_owned()),
                simplify: Some(PixelThreshold {
                    below_max_zoom: 2.0,
                    at_max_zoom: 2.0,
                }),
                ..RuleDef::default()
            }],
            ..LayerDef::new("roads", 0..=1, GRID)
        }],
    }])
    .unwrap();
    let line = |id, class: &str| SourceFeature {
        id: Some(id),
        geometry: Geometry::Lines(vec![LineString::from(vec![(10.0, 10.0), (30.0, 10.5)])]),
        props: vec![(KeyId::new(0), Prop::Str(class.to_owned()))],
    };
    let features = [line(1, "detailed"), line(2, "smooth")];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"
    LayerFeature { layer: "roads", id: Some(1), zooms: 0..=1, simplify: PixelThreshold { below_max_zoom: 0.0, at_max_zoom: 0.0 }, min_size: PixelThreshold { below_max_zoom: 0.0, at_max_zoom: 0.0 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "roads", id: Some(2), zooms: 0..=1, simplify: PixelThreshold { below_max_zoom: 2.0, at_max_zoom: 2.0 }, min_size: PixelThreshold { below_max_zoom: 0.0, at_max_zoom: 0.0 }, props: {}, bands: [], sort: None }
    "#);
}

#[test]
fn rule_attributes_replace_and_add_to_the_layers() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("name".to_owned(), None),
            ColumnDef::Value("class".to_owned(), None),
            ColumnDef::Value("rank".to_owned(), None),
        ],
        layers: vec![LayerDef {
            attributes: AttributesDef::Computed {
                attributes: vec![
                    ComputedAttr {
                        name: "kind".to_owned(),
                        value: ValueDef::Expr("class".to_owned()),
                        zooms: 0..=30,
                    },
                    ComputedAttr {
                        name: "label".to_owned(),
                        value: ValueDef::Expr("name".to_owned()),
                        zooms: 0..=30,
                    },
                ],
                prefixes: vec![],
            },
            rules: vec![RuleDef {
                when: Some("class == 'river'".to_owned()),
                attributes: vec![
                    ComputedAttr {
                        name: "kind".to_owned(),
                        value: ValueDef::Literal(Prop::Str("waterway".to_owned())),
                        zooms: 0..=30,
                    },
                    ComputedAttr {
                        name: "big".to_owned(),
                        value: ValueDef::Expr("rank * 2 > 10".to_owned()),
                        zooms: 0..=1,
                    },
                ],
                ..RuleDef::default()
            }],
            ..LayerDef::new("water", 0..=2, GRID)
        }],
    }])
    .unwrap();
    let point = |id, name: &str, class: &str, rank| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![
            (KeyId::new(0), Prop::Str(name.to_owned())),
            (KeyId::new(1), Prop::Str(class.to_owned())),
            (KeyId::new(2), Prop::I64(rank)),
        ],
    };
    let features = [point(1, "Rhein", "river", 7), point(2, "Bodensee", "lake", 9)];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"
    LayerFeature { layer: "water", id: Some(1), zooms: 0..=2, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {"label": Str("Rhein"), "kind": Str("waterway"), "big": Bool(true)}, bands: [(2, {"label": Str("Rhein"), "kind": Str("waterway")})], sort: None }
    LayerFeature { layer: "water", id: Some(2), zooms: 0..=2, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {"label": Str("Bodensee"), "kind": Str("lake")}, bands: [], sort: None }
    "#);
}

#[test]
fn a_failing_rule_condition_does_not_match_and_is_counted() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("lanes".to_owned(), None)],
        layers: vec![LayerDef {
            minzoom_expr: Some("1".to_owned()),
            attributes: AttributesDef::None,
            rules: vec![RuleDef {
                when: Some("8 / lanes >= 2".to_owned()),
                minzoom: Some(ZoomDef::Fixed(0)),
                ..RuleDef::default()
            }],
            ..LayerDef::new("roads", 0..=1, GRID)
        }],
    }])
    .unwrap();
    let point = |id, lanes| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![(KeyId::new(0), Prop::I64(lanes))],
    };
    let features = [point(1, 4), point(2, 0), point(3, 0)];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"
    LayerFeature { layer: "roads", id: Some(1), zooms: 0..=1, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "roads", id: Some(2), zooms: 1..=1, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "roads", id: Some(3), zooms: 1..=1, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: None }
    ExprErrors { layer: "roads", expr: "8 / lanes >= 2", errors: 2 }
    "#);
}

#[test]
fn a_rule_without_a_condition_or_expressions_still_applies() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![
            LayerDef {
                simplify: PixelThreshold::ZERO,
                min_size: PixelThreshold::ZERO,
                ..LayerDef::new("plain", 0..=0, GRID)
            },
            LayerDef {
                simplify: PixelThreshold::ZERO,
                min_size: PixelThreshold::ZERO,
                rules: vec![RuleDef {
                    simplify: Some(PixelThreshold {
                        below_max_zoom: 2.0,
                        at_max_zoom: 2.0,
                    }),
                    ..RuleDef::default()
                }],
                ..LayerDef::new("ruled", 0..=0, GRID)
            },
        ],
    }])
    .unwrap();
    let features = [SourceFeature {
        id: Some(1),
        geometry: Geometry::Lines(vec![LineString::from(vec![(10.0, 10.0), (30.0, 10.5)])]),
        props: vec![],
    }];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"
    LayerFeature { layer: "plain", id: Some(1), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 0.0, at_max_zoom: 0.0 }, min_size: PixelThreshold { below_max_zoom: 0.0, at_max_zoom: 0.0 }, props: {}, bands: [], sort: None }
    LayerFeature { layer: "ruled", id: Some(1), zooms: 0..=0, simplify: PixelThreshold { below_max_zoom: 2.0, at_max_zoom: 2.0 }, min_size: PixelThreshold { below_max_zoom: 0.0, at_max_zoom: 0.0 }, props: {}, bands: [], sort: None }
    "#);
}

#[test]
fn a_rule_attribute_reads_its_column_after_unused_columns_are_dropped() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("class".to_owned(), None),
            ColumnDef::Value("name".to_owned(), None),
        ],
        layers: vec![LayerDef {
            attributes: AttributesDef::None,
            rules: vec![RuleDef {
                attributes: vec![ComputedAttr {
                    name: "label".to_owned(),
                    value: ValueDef::Expr("name".to_owned()),
                    zooms: 1..=30,
                }],
                ..RuleDef::default()
            }],
            ..LayerDef::new("water", 0..=1, GRID)
        }],
    }])
    .unwrap();
    assert_eq!(plan.columns(0), ["name"]);
    let features = [SourceFeature {
        id: Some(1),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![(KeyId::new(0), Prop::Str("Rhein".to_owned()))],
    }];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"LayerFeature { layer: "water", id: Some(1), zooms: 0..=1, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [(1, {"label": Str("Rhein")})], sort: None }"#);
}

#[test]
fn sort_by_applies_whichever_rule_a_feature_matches() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("rank".to_owned(), None)],
        layers: vec![LayerDef {
            attributes: AttributesDef::None,
            rules: vec![
                RuleDef {
                    when: Some("rank > 5".to_owned()),
                    attributes: vec![ComputedAttr {
                        name: "kind".to_owned(),
                        value: ValueDef::Literal(Prop::Str("major".to_owned())),
                        zooms: 0..=30,
                    }],
                    ..RuleDef::default()
                },
                RuleDef {
                    minzoom: Some(ZoomDef::Fixed(1)),
                    ..RuleDef::default()
                },
            ],
            sort_by: vec![SortDef {
                expr: "rank".to_owned(),
                descending: true,
            }],
            ..LayerDef::new("places", 0..=1, GRID)
        }],
    }])
    .unwrap();
    let point = |id, rank| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![(KeyId::new(0), Prop::I64(rank))],
    };
    let features = [point(1, 2), point(2, 9), point(3, 1), point(4, 7)];
    insta::assert_snapshot!(evaluate(&plan, &features), @r#"
    LayerFeature { layer: "places", id: Some(1), zooms: 1..=1, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: Some(fcfffffffffffffffd) }
    LayerFeature { layer: "places", id: Some(2), zooms: 0..=1, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {"kind": Str("major")}, bands: [], sort: Some(fcfffffffffffffff6) }
    LayerFeature { layer: "places", id: Some(3), zooms: 1..=1, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {}, bands: [], sort: Some(fcfffffffffffffffe) }
    LayerFeature { layer: "places", id: Some(4), zooms: 0..=1, simplify: PixelThreshold { below_max_zoom: 0.1, at_max_zoom: 0.0625 }, min_size: PixelThreshold { below_max_zoom: 1.0, at_max_zoom: 0.0625 }, props: {"kind": Str("major")}, bands: [], sort: Some(fcfffffffffffffff8) }
    "#);
}

#[test]
fn a_table_reads_the_rows_of_each_layers_geometry() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![
            LayerDef {
                geometry: Some(GeometryType::Point),
                ..LayerDef::new("stops", 0..=0, GRID)
            },
            LayerDef {
                geometry: Some(GeometryType::Line),
                ..LayerDef::new("roads", 0..=0, GRID)
            },
        ],
    }])
    .unwrap();
    insta::assert_debug_snapshot!(plan.row_filter(0, 0..=0), @"
    Matching(
        Or(
            [
                Geometry(
                    Point,
                ),
                Geometry(
                    Line,
                ),
            ],
        ),
    )
    ");
}

#[test]
fn a_layer_without_a_filter_reads_every_row() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![
            LayerDef {
                geometry: Some(GeometryType::Line),
                ..LayerDef::new("roads", 0..=0, GRID)
            },
            LayerDef::new("all", 0..=0, GRID),
        ],
    }])
    .unwrap();
    insta::assert_debug_snapshot!(plan.row_filter(0, 0..=0), @"All");
}

#[test]
fn a_table_reads_only_the_rows_of_layers_at_the_zooms_read() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![
            LayerDef {
                geometry: Some(GeometryType::Line),
                ..LayerDef::new("detail", 10..=14, GRID)
            },
            LayerDef {
                geometry: Some(GeometryType::Polygon),
                ..LayerDef::new("overview", 0..=14, GRID)
            },
        ],
    }])
    .unwrap();
    insta::assert_debug_snapshot!(plan.row_filter(0, 5..=5), @"
    Matching(
        Geometry(
            Polygon,
        ),
    )
    ");
    insta::assert_debug_snapshot!(plan.row_filter(0, 9..=10), @"
    Matching(
        Or(
            [
                Geometry(
                    Line,
                ),
                Geometry(
                    Polygon,
                ),
            ],
        ),
    )
    ");
}

#[test]
fn a_table_reads_the_rows_some_layer_takes() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("class".to_owned(), Some(ValueKind::String)),
            ColumnDef::Value("rank".to_owned(), Some(ValueKind::Int)),
        ],
        layers: vec![
            LayerDef {
                filter: Some("class != 'service'".to_owned()),
                ..LayerDef::new("roads", 0..=0, GRID)
            },
            LayerDef {
                geometry: Some(GeometryType::Point),
                filter: Some("!(rank >= 4)".to_owned()),
                ..LayerDef::new("minor", 0..=0, GRID)
            },
        ],
    }])
    .unwrap();
    insta::assert_debug_snapshot!(plan.row_filter(0, 0..=0), @r#"
    Matching(
        Or(
            [
                Distinct {
                    left: Column(
                        "class",
                    ),
                    right: Literal(
                        String(
                            "service",
                        ),
                    ),
                    kind: String,
                    polarity: AsWritten,
                },
                And(
                    [
                        Geometry(
                            Point,
                        ),
                        Compare {
                            left: Column(
                                "rank",
                            ),
                            op: Lt,
                            right: Literal(
                                Int(
                                    4,
                                ),
                            ),
                            kind: Int,
                        },
                    ],
                ),
            ],
        ),
    )
    "#);
}

#[test]
fn a_where_with_no_sql_equivalent_is_untranslatable() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("name".to_owned(), Some(ValueKind::String))],
        layers: vec![LayerDef {
            filter: Some("name + '!' == 'A!'".to_owned()),
            ..LayerDef::new("named", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let rows = plan.row_filter(0, 0..=0);
    insta::assert_debug_snapshot!(rows, @r#"
    Untranslatable(
        Untranslatable {
            layer: "named",
            expr: "name + '!' == 'A!'",
            reason: Function(
                "+",
            ),
        },
    )
    "#);
    let RowFilter::Untranslatable(err) = rows else {
        panic!("translatable");
    };
    insta::assert_snapshot!(err, @"layer `named`: `name + '!' == 'A!'` uses `+`, which has no SQL equivalent");
}

#[test]
fn a_layer_reading_every_row_does_not_hide_an_untranslatable_where() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("name".to_owned(), Some(ValueKind::String))],
        layers: vec![
            LayerDef::new("all", 0..=0, GRID),
            LayerDef {
                filter: Some("name + '!' == 'A!'".to_owned()),
                ..LayerDef::new("named", 0..=0, GRID)
            },
        ],
    }])
    .unwrap();
    insta::assert_debug_snapshot!(plan.row_filter(0, 0..=0), @r#"
    Untranslatable(
        Untranslatable {
            layer: "named",
            expr: "name + '!' == 'A!'",
            reason: Function(
                "+",
            ),
        },
    )
    "#);
}

#[test]
fn a_column_of_unknown_type_is_not_compared() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("code".to_owned(), None)],
        layers: vec![LayerDef {
            filter: Some("code == 'A'".to_owned()),
            ..LayerDef::new("coded", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let rows = plan.row_filter(0, 0..=0);
    insta::assert_debug_snapshot!(rows, @r#"
    Untranslatable(
        Untranslatable {
            layer: "coded",
            expr: "code == 'A'",
            reason: UnknownColumnType(
                "code",
            ),
        },
    )
    "#);
    let RowFilter::Untranslatable(err) = rows else {
        panic!("translatable");
    };
    insta::assert_snapshot!(err, @"layer `coded`: `code == 'A'` compares `code`, whose type the source cannot compare");
}

#[test]
fn lower_ascii_as_a_global_function_is_untranslatable() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("name".to_owned(), Some(ValueKind::String))],
        layers: vec![LayerDef {
            filter: Some("lowerAscii(name) == 'a'".to_owned()),
            ..LayerDef::new("named", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let RowFilter::Untranslatable(err) = plan.row_filter(0, 0..=0) else {
        panic!("translatable");
    };
    insta::assert_snapshot!(err, @"layer `named`: `lowerAscii(name) == 'a'` uses `lowerAscii`, which has no SQL equivalent");
}

#[test]
fn starts_with_a_column_is_untranslatable() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("name".to_owned(), Some(ValueKind::String)),
            ColumnDef::Value("other".to_owned(), Some(ValueKind::String)),
        ],
        layers: vec![LayerDef {
            filter: Some("name.startsWith(other)".to_owned()),
            ..LayerDef::new("named", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let RowFilter::Untranslatable(err) = plan.row_filter(0, 0..=0) else {
        panic!("translatable");
    };
    insta::assert_snapshot!(err, @"layer `named`: `name.startsWith(other)` calls `startsWith` without a literal");
}

#[test]
fn size_of_an_int_column_is_untranslatable() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("rank".to_owned(), Some(ValueKind::Int))],
        layers: vec![LayerDef {
            filter: Some("size(rank) == 1".to_owned()),
            ..LayerDef::new("named", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let RowFilter::Untranslatable(err) = plan.row_filter(0, 0..=0) else {
        panic!("translatable");
    };
    insta::assert_snapshot!(err, @"layer `named`: `size(rank) == 1` calls `size` on something other than a string column");
}

#[test]
fn a_string_function_compared_with_a_column_is_untranslatable() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("name".to_owned(), Some(ValueKind::String)),
            ColumnDef::Value("other".to_owned(), Some(ValueKind::String)),
        ],
        layers: vec![LayerDef {
            filter: Some("name.lowerAscii() == other".to_owned()),
            ..LayerDef::new("named", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let RowFilter::Untranslatable(err) = plan.row_filter(0, 0..=0) else {
        panic!("translatable");
    };
    insta::assert_snapshot!(err, @"layer `named`: `name.lowerAscii() == other` compares a function of a column to a column");
}

#[test]
fn a_where_is_not_translated_while_a_keys_column_is_scanned() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("class".to_owned(), Some(ValueKind::String)),
            ColumnDef::Keys("tags".to_owned()),
        ],
        layers: vec![LayerDef {
            filter: Some("class == 'primary'".to_owned()),
            ..LayerDef::new("primary", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let rows = plan.row_filter(0, 0..=0);
    insta::assert_debug_snapshot!(rows, @r#"
    Untranslatable(
        Untranslatable {
            layer: "primary",
            expr: "class == 'primary'",
            reason: KeysMayReplaceColumns(
                "tags",
            ),
        },
    )
    "#);
    let RowFilter::Untranslatable(err) = rows else {
        panic!("translatable");
    };
    insta::assert_snapshot!(err, @"layer `primary`: `class == 'primary'` reads columns that a key of `tags` may replace (list `attributes` without its keys)");
}

#[test]
fn an_int_column_compares_with_each_number_of_a_list_by_its_own_type() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("rank".to_owned(), Some(ValueKind::Int))],
        layers: vec![LayerDef {
            filter: Some("rank in [0.5, 9007199254740993]".to_owned()),
            ..LayerDef::new("ranked", 0..=0, GRID)
        }],
    }])
    .unwrap();
    insta::assert_debug_snapshot!(plan.row_filter(0, 0..=0), @r#"
    Matching(
        Or(
            [
                In {
                    term: Column(
                        "rank",
                    ),
                    values: [
                        Float(
                            0.5,
                        ),
                    ],
                    kind: Float,
                    polarity: AsWritten,
                },
                In {
                    term: Column(
                        "rank",
                    ),
                    values: [
                        Int(
                            9007199254740993,
                        ),
                    ],
                    kind: Int,
                    polarity: AsWritten,
                },
            ],
        ),
    )
    "#);
}
