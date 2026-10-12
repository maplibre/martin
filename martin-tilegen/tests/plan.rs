//! What `Plan::new` accepts and rejects, and which columns it scans.

use martin_core::generate::props::Prop;
use martin_tilegen::LayerGrid;
use martin_tilegen::plan::{
    AttributesDef, ColumnDef, ComputedAttr, IdDef, LayerDef, Plan, PlanError, RuleDef, TableDef,
    ValueDef,
};

const GRID: LayerGrid = LayerGrid {
    extent: 4096,
    buffer: 64,
};

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
        layers: (0..257)
            .map(|i| LayerDef::new(format!("l{i}"), 0..=0, GRID))
            .collect(),
    }])
    .unwrap_err();
    assert!(matches!(err, PlanError::TooManyLayers(257)), "{err}");
}

#[test]
fn accepts_256_layers() {
    Plan::new(vec![TableDef {
        columns: vec![],
        layers: (0..256)
            .map(|i| LayerDef::new(format!("l{i}"), 0..=0, GRID))
            .collect(),
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
        columns: vec![ColumnDef::Value("name".to_owned())],
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
}

#[test]
fn rejects_a_repeated_attribute_column() {
    let err = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("name".to_owned())],
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
        dynamic_props: false,
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
        columns: vec![ColumnDef::Value("class".to_owned())],
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
            ColumnDef::Value("class".to_owned()),
            ColumnDef::Keys("tags".to_owned()),
            ColumnDef::Value("name".to_owned()),
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
            ColumnDef::Value("class".to_owned()),
            ColumnDef::Value("name".to_owned()),
            ColumnDef::Value("rank".to_owned()),
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
            ColumnDef::Value("name".to_owned()),
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
            ColumnDef::Value("class".to_owned()),
            ColumnDef::Value("gid".to_owned()),
            ColumnDef::Value("lanes".to_owned()),
            ColumnDef::Value("name".to_owned()),
            ColumnDef::Value("rank".to_owned()),
            ColumnDef::Value("surface".to_owned()),
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
            ColumnDef::Value("name".to_owned()),
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
            ColumnDef::Value("name".to_owned()),
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
            ColumnDef::Value("lang".to_owned()),
            ColumnDef::Value("name:de".to_owned()),
            ColumnDef::Value("name:en".to_owned()),
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
            ColumnDef::Value("tag_count".to_owned()),
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
                ColumnDef::Value("class".to_owned()),
                ColumnDef::Value("name".to_owned()),
            ],
            layers: vec![LayerDef {
                attributes: AttributesDef::Columns(vec!["class".to_owned()]),
                ..LayerDef::new("roads", 0..=14, GRID)
            }],
        },
        TableDef {
            columns: vec![
                ColumnDef::Value("class".to_owned()),
                ColumnDef::Value("name".to_owned()),
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
