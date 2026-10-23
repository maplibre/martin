//! Which paths through the layers a feature takes, as the plan evaluates it.

use geo_types::{Coord, LineString};
use martin_tilegen::plan::{
    AttributesDef, ColumnDef, ColumnValues, ComputedAttr, GeometryType, LayerDef, Literal, Plan, RuleDef, TableDef,
    ValueDef, ValueKind,
};
use martin_tilegen::props::{KeyId, Prop};
use martin_tilegen::source::{Crs, Geometry, SourceFeature};
use martin_tilegen::{LayerGrid, PixelThreshold};

const GRID: LayerGrid = LayerGrid {
    extent: 4096,
    buffer: 64,
};

#[test]
fn a_city_takes_the_first_rule() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            rules: vec![
                RuleDef {
                    when: Some("class == 'city'".to_owned()),
                    ..RuleDef::default()
                },
                RuleDef {
                    when: Some("class.startsWith('c')".to_owned()),
                    ..RuleDef::default()
                },
            ],
            ..LayerDef::new("places", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let city = SourceFeature {
        id: Some(1),
        geometry: Geometry::Point(Coord { x: 0.01, y: 0.01 }),
        props: vec![(KeyId::new(0), Prop::Str("city".into()))],
    };
    let paths = plan.paths_of(0, &[], Crs::Wgs84, &city).unwrap();
    let taken: Vec<String> = paths.iter().map(ToString::to_string).collect();
    insta::assert_debug_snapshot!(taken, @r#"
    [
        "places: rule 1 `where: class == 'city'`",
    ]
    "#);
}

#[test]
fn a_capital_skips_the_first_rule_and_takes_the_second() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            rules: vec![
                RuleDef {
                    when: Some("class == 'city'".to_owned()),
                    ..RuleDef::default()
                },
                RuleDef {
                    when: Some("class.startsWith('c')".to_owned()),
                    ..RuleDef::default()
                },
            ],
            ..LayerDef::new("places", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let capital = SourceFeature {
        id: Some(2),
        geometry: Geometry::Point(Coord { x: 0.01, y: 0.01 }),
        props: vec![(KeyId::new(0), Prop::Str("capital".into()))],
    };
    let paths = plan.paths_of(0, &[], Crs::Wgs84, &capital).unwrap();
    let taken: Vec<String> = paths.iter().map(ToString::to_string).collect();
    insta::assert_debug_snapshot!(taken, @r#"
    [
        "places: rule 2 `where: class.startsWith('c')`",
    ]
    "#);
}

#[test]
fn a_place_without_a_class_takes_no_rule() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            rules: vec![
                RuleDef {
                    when: Some("class == 'city'".to_owned()),
                    ..RuleDef::default()
                },
                RuleDef {
                    when: Some("class.startsWith('c')".to_owned()),
                    ..RuleDef::default()
                },
            ],
            ..LayerDef::new("places", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let unnamed = SourceFeature {
        id: Some(3),
        geometry: Geometry::Point(Coord { x: 0.01, y: 0.01 }),
        props: vec![],
    };
    let paths = plan.paths_of(0, &[], Crs::Wgs84, &unnamed).unwrap();
    let taken: Vec<String> = paths.iter().map(ToString::to_string).collect();
    insta::assert_debug_snapshot!(taken, @r#"
    [
        "places: no rule matches",
    ]
    "#);
}

#[test]
fn a_place_above_the_population_is_where() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("population".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            filter: Some("population > 1000".to_owned()),
            ..LayerDef::new("places", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let big = SourceFeature {
        id: Some(1),
        geometry: Geometry::Point(Coord { x: 0.01, y: 0.01 }),
        props: vec![(KeyId::new(0), Prop::I64(5000))],
    };
    let paths = plan.paths_of(0, &[], Crs::Wgs84, &big).unwrap();
    let taken: Vec<String> = paths.iter().map(ToString::to_string).collect();
    insta::assert_debug_snapshot!(taken, @r#"
    [
        "places: kept by `where: population > 1000`",
    ]
    "#);
}

#[test]
fn a_place_below_the_population_is_not_where() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("population".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            filter: Some("population > 1000".to_owned()),
            ..LayerDef::new("places", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let small = SourceFeature {
        id: Some(2),
        geometry: Geometry::Point(Coord { x: 0.01, y: 0.01 }),
        props: vec![(KeyId::new(0), Prop::I64(10))],
    };
    let paths = plan.paths_of(0, &[], Crs::Wgs84, &small).unwrap();
    let taken: Vec<String> = paths.iter().map(ToString::to_string).collect();
    insta::assert_debug_snapshot!(taken, @r#"
    [
        "places: dropped, `where: population > 1000` is false",
    ]
    "#);
}

#[test]
fn a_place_without_a_population_is_where_with_the_column_null() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("population".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            filter: Some("population > 1000".to_owned()),
            ..LayerDef::new("places", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let unknown = SourceFeature {
        id: Some(3),
        geometry: Geometry::Point(Coord { x: 0.01, y: 0.01 }),
        props: vec![],
    };
    let paths = plan.paths_of(0, &[], Crs::Wgs84, &unknown).unwrap();
    let taken: Vec<String> = paths.iter().map(ToString::to_string).collect();
    insta::assert_debug_snapshot!(taken, @r#"
    [
        "places: dropped, `population` is null in `where: population > 1000`",
    ]
    "#);
}

#[test]
fn a_city_without_an_english_name_is_labelled_by_its_name() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("class".to_owned(), None),
            ColumnDef::Value("name".to_owned(), None),
            ColumnDef::Value("name_en".to_owned(), None),
        ],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            attributes: AttributesDef::None,
            rules: vec![RuleDef {
                when: Some("class == 'city'".to_owned()),
                attributes: vec![ComputedAttr {
                    name: "label".to_owned(),
                    value: ValueDef::Expr("coalesce(name_en, name)".to_owned()),
                    zooms: 0..=14,
                }],
                ..RuleDef::default()
            }],
            ..LayerDef::new("places", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let city = SourceFeature {
        id: Some(1),
        geometry: Geometry::Point(Coord { x: 0.01, y: 0.01 }),
        props: vec![
            (KeyId::new(0), Prop::Str("city".into())),
            (KeyId::new(1), Prop::Str("Wien".into())),
        ],
    };
    let paths = plan.paths_of(0, &[], Crs::Wgs84, &city).unwrap();
    let taken: Vec<String> = paths.iter().map(ToString::to_string).collect();
    insta::assert_debug_snapshot!(taken, @r#"
    [
        "places: rule 1 `where: class == 'city'`",
        "places: rule 1 attributes.label from `name`",
    ]
    "#);
}

#[test]
fn a_town_outside_the_rule_takes_none_of_the_choices_of_its_attributes() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("class".to_owned(), None),
            ColumnDef::Value("name".to_owned(), None),
            ColumnDef::Value("name_en".to_owned(), None),
        ],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            attributes: AttributesDef::None,
            rules: vec![RuleDef {
                when: Some("class == 'city'".to_owned()),
                attributes: vec![ComputedAttr {
                    name: "label".to_owned(),
                    value: ValueDef::Expr("coalesce(name_en, name)".to_owned()),
                    zooms: 0..=14,
                }],
                ..RuleDef::default()
            }],
            ..LayerDef::new("places", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let town = SourceFeature {
        id: Some(2),
        geometry: Geometry::Point(Coord { x: 0.01, y: 0.01 }),
        props: vec![
            (KeyId::new(0), Prop::Str("town".into())),
            (KeyId::new(1), Prop::Str("Baden".into())),
        ],
    };
    let paths = plan.paths_of(0, &[], Crs::Wgs84, &town).unwrap();
    let taken: Vec<String> = paths.iter().map(ToString::to_string).collect();
    insta::assert_debug_snapshot!(taken, @r#"
    [
        "places: no rule matches",
    ]
    "#);
}

#[test]
fn a_known_column_the_layer_reads_has_a_path_per_value() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("type".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            attributes: AttributesDef::Computed {
                attributes: vec![ComputedAttr {
                    name: "kind".to_owned(),
                    value: ValueDef::Expr("type.upperAscii()".to_owned()),
                    zooms: 0..=14,
                }],
                prefixes: vec![],
            },
            ..LayerDef::new("devices", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let known = [ColumnValues {
        column: "type".to_owned(),
        values: vec![Literal::String("stop".to_owned()), Literal::String("yield".to_owned())],
    }];
    insta::assert_debug_snapshot!(plan.branches(0, &known).unwrap(), @r#"
    [
        Branch {
            layer: "devices",
            path: "type 'stop' -> attributes.kind 'STOP'",
            rows: Ok(
                And(
                    [
                        Geometry(
                            Point,
                        ),
                        Compare {
                            left: Column(
                                "type",
                            ),
                            op: Eq,
                            right: Literal(
                                String(
                                    "stop",
                                ),
                            ),
                            kind: String,
                        },
                    ],
                ),
            ),
            size: None,
        },
        Branch {
            layer: "devices",
            path: "type 'yield' -> attributes.kind 'YIELD'",
            rows: Ok(
                And(
                    [
                        Geometry(
                            Point,
                        ),
                        Compare {
                            left: Column(
                                "type",
                            ),
                            op: Eq,
                            right: Literal(
                                String(
                                    "yield",
                                ),
                            ),
                            kind: String,
                        },
                    ],
                ),
            ),
            size: None,
        },
    ]
    "#);
}

#[test]
fn a_known_column_the_layer_does_not_read_has_no_path() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("type".to_owned(), None),
            ColumnDef::Value("name".to_owned(), None),
        ],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            attributes: AttributesDef::Computed {
                attributes: vec![ComputedAttr {
                    name: "label".to_owned(),
                    value: ValueDef::Expr("name.upperAscii()".to_owned()),
                    zooms: 0..=14,
                }],
                prefixes: vec![],
            },
            ..LayerDef::new("devices", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let known = [ColumnValues {
        column: "type".to_owned(),
        values: vec![Literal::String("stop".to_owned()), Literal::String("yield".to_owned())],
    }];
    let branches: Vec<String> = plan
        .branches(0, &known)
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    insta::assert_debug_snapshot!(branches, @r#"
    [
        "devices: every feature",
    ]
    "#);
}

#[test]
fn a_known_column_in_a_where_has_no_null_path() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("type".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            filter: Some("type == 'stop'".to_owned()),
            ..LayerDef::new("devices", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let known = [ColumnValues {
        column: "type".to_owned(),
        values: vec![Literal::String("stop".to_owned()), Literal::String("yield".to_owned())],
    }];
    let branches: Vec<String> = plan
        .branches(0, &known)
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    insta::assert_debug_snapshot!(branches, @r#"
    [
        "devices: type 'stop' -> kept by `where: type == 'stop'`",
        "devices: type 'yield' -> dropped, `where: type == 'stop'` is false",
    ]
    "#);
}

#[test]
fn a_value_that_decides_every_rule_names_the_rule_it_takes() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("type".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            rules: vec![RuleDef {
                when: Some("type == 'stop'".to_owned()),
                attributes: vec![ComputedAttr {
                    name: "label".to_owned(),
                    value: ValueDef::Expr("type + '!'".to_owned()),
                    zooms: 0..=14,
                }],
                ..RuleDef::default()
            }],
            ..LayerDef::new("devices", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let known = [ColumnValues {
        column: "type".to_owned(),
        values: vec![Literal::String("stop".to_owned()), Literal::String("yield".to_owned())],
    }];
    let branches: Vec<String> = plan
        .branches(0, &known)
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    insta::assert_debug_snapshot!(branches, @r#"
    [
        "devices: type 'stop' -> rule 1, attributes.label 'stop!'",
        "devices: type 'yield' -> no rule matches",
    ]
    "#);
}

#[test]
fn a_rule_the_values_cannot_decide_keeps_its_paths() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("type".to_owned(), None),
            ColumnDef::Value("lanes".to_owned(), None),
        ],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            attributes: AttributesDef::Computed {
                attributes: vec![ComputedAttr {
                    name: "kind".to_owned(),
                    value: ValueDef::Expr("type.upperAscii()".to_owned()),
                    zooms: 0..=14,
                }],
                prefixes: vec![],
            },
            rules: vec![RuleDef {
                when: Some("lanes > 2".to_owned()),
                ..RuleDef::default()
            }],
            ..LayerDef::new("devices", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let known = [ColumnValues {
        column: "type".to_owned(),
        values: vec![Literal::String("stop".to_owned())],
    }];
    let branches: Vec<String> = plan
        .branches(0, &known)
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    insta::assert_debug_snapshot!(branches, @r#"
    [
        "devices: rule 1 `where: lanes > 2`",
        "devices: no rule matches",
        "devices: type 'stop' -> attributes.kind 'STOP'",
    ]
    "#);
}

#[test]
fn a_choice_inside_an_expression_names_its_field_without_a_value() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("lanes".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            attributes: AttributesDef::Computed {
                attributes: vec![ComputedAttr {
                    name: "rank".to_owned(),
                    value: ValueDef::Expr("(lanes > 2 ? 1 : 0) + 1".to_owned()),
                    zooms: 0..=14,
                }],
                prefixes: vec![],
            },
            ..LayerDef::new("roads", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let branches: Vec<String> = plan.branches(0, &[]).unwrap().iter().map(ToString::to_string).collect();
    insta::assert_debug_snapshot!(branches, @r#"
    [
        "roads: attributes.rank, `lanes > 2` is true",
        "roads: attributes.rank, `lanes > 2` is false",
        "roads: attributes.rank, `lanes` is null in `lanes > 2`",
    ]
    "#);
}

#[test]
fn a_value_path_cannot_select_rows_when_a_json_column_may_replace_the_column() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("type".to_owned(), None),
            ColumnDef::Keys("tags".to_owned()),
        ],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            filter: Some("type == 'stop'".to_owned()),
            ..LayerDef::new("devices", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let known = [ColumnValues {
        column: "type".to_owned(),
        values: vec![Literal::String("stop".to_owned())],
    }];
    insta::assert_debug_snapshot!(plan.branches(0, &known).unwrap(), @r#"
    [
        Branch {
            layer: "devices",
            path: "type 'stop' -> kept by `where: type == 'stop'`",
            rows: Err(
                Untranslatable {
                    layer: "devices",
                    expr: "the source's filter",
                    reason: KeysMayReplaceColumns(
                        "tags",
                    ),
                },
            ),
            size: None,
        },
    ]
    "#);
}

#[test]
fn a_column_read_as_text_has_no_path_for_a_number() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("amount".to_owned(), Some(ValueKind::String))],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            filter: Some("amount != ''".to_owned()),
            ..LayerDef::new("payments", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let known = [ColumnValues {
        column: "amount".to_owned(),
        values: vec![Literal::Int(2)],
    }];
    let branches: Vec<String> = plan
        .branches(0, &known)
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    insta::assert_debug_snapshot!(branches, @r#"
    [
        "payments: kept by `where: amount != ''`",
        "payments: dropped, `where: amount != ''` is false",
    ]
    "#);
}

#[test]
fn a_float_column_compares_a_whole_number_as_a_float() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("height".to_owned(), Some(ValueKind::Float))],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            filter: Some("height > 1.0".to_owned()),
            ..LayerDef::new("poles", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let known = [ColumnValues {
        column: "height".to_owned(),
        values: vec![Literal::Int(2)],
    }];
    let branches = plan.branches(0, &known).unwrap();
    insta::assert_debug_snapshot!(branches.last().unwrap(), @r#"
    Branch {
        layer: "poles",
        path: "height 2 -> kept by `where: height > 1.0`",
        rows: Ok(
            And(
                [
                    Geometry(
                        Point,
                    ),
                    Compare {
                        left: Column(
                            "height",
                        ),
                        op: Eq,
                        right: Literal(
                            Int(
                                2,
                            ),
                        ),
                        kind: Float,
                    },
                ],
            ),
        ),
        size: None,
    }
    "#);
}

#[test]
fn a_stop_sign_takes_the_path_of_its_value() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("type".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            attributes: AttributesDef::Computed {
                attributes: vec![ComputedAttr {
                    name: "kind".to_owned(),
                    value: ValueDef::Expr("type.upperAscii()".to_owned()),
                    zooms: 0..=14,
                }],
                prefixes: vec![],
            },
            ..LayerDef::new("devices", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let known = [ColumnValues {
        column: "type".to_owned(),
        values: vec![Literal::String("stop".to_owned()), Literal::String("yield".to_owned())],
    }];
    let stop = SourceFeature {
        id: Some(1),
        geometry: Geometry::Point(Coord { x: 0.01, y: 0.01 }),
        props: vec![(KeyId::new(0), Prop::Str("stop".into()))],
    };
    let paths = plan.paths_of(0, &known, Crs::Wgs84, &stop).unwrap();
    let taken: Vec<String> = paths.iter().map(ToString::to_string).collect();
    insta::assert_debug_snapshot!(taken, @r#"
    [
        "devices: type 'stop' -> attributes.kind 'STOP'",
    ]
    "#);
}

#[test]
fn a_value_the_filter_does_not_list_takes_no_path() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("type".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            attributes: AttributesDef::Computed {
                attributes: vec![ComputedAttr {
                    name: "kind".to_owned(),
                    value: ValueDef::Expr("type.upperAscii()".to_owned()),
                    zooms: 0..=14,
                }],
                prefixes: vec![],
            },
            ..LayerDef::new("devices", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let known = [ColumnValues {
        column: "type".to_owned(),
        values: vec![Literal::String("stop".to_owned()), Literal::String("yield".to_owned())],
    }];
    let give_way = SourceFeature {
        id: Some(1),
        geometry: Geometry::Point(Coord { x: 0.01, y: 0.01 }),
        props: vec![(KeyId::new(0), Prop::Str("give_way".into()))],
    };
    let paths = plan.paths_of(0, &known, Crs::Wgs84, &give_way).unwrap();
    let taken: Vec<String> = paths.iter().map(ToString::to_string).collect();
    insta::assert_debug_snapshot!(taken, @"[]");
}

#[test]
fn an_integer_property_takes_the_path_of_its_known_integer_value() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("lanes".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Line),
            filter: Some("lanes > 1".to_owned()),
            ..LayerDef::new("roads", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let known = [ColumnValues {
        column: "lanes".to_owned(),
        values: vec![Literal::Int(1), Literal::Int(2)],
    }];
    let two_lanes = SourceFeature {
        id: Some(1),
        geometry: Geometry::Line(LineString::from(vec![(0.01, 0.01), (0.02, 0.01)])),
        props: vec![(KeyId::new(0), Prop::I64(2))],
    };
    let paths = plan.paths_of(0, &known, Crs::Wgs84, &two_lanes).unwrap();
    let taken: Vec<String> = paths.iter().map(ToString::to_string).collect();
    insta::assert_debug_snapshot!(taken, @r#"
    [
        "roads: lanes 2 -> kept by `where: lanes > 1`",
    ]
    "#);
}

#[test]
fn a_rule_with_its_own_min_size_has_its_own_size_paths() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Line),
            rules: vec![RuleDef {
                when: Some("class == 'major'".to_owned()),
                min_size: Some(PixelThreshold::ZERO),
                ..RuleDef::default()
            }],
            ..LayerDef::new("roads", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let branches: Vec<String> = plan.branches(0, &[]).unwrap().iter().map(ToString::to_string).collect();
    insta::assert_debug_snapshot!(branches, @r#"
    [
        "roads: rule 1 `where: class == 'major'`",
        "roads: no rule matches",
        "roads: no rule: kept only at z14 (`min_size_at_maxzoom`)",
        "roads: no rule: dropped, smaller than `min_size` at every zoom",
    ]
    "#);
}

#[test]
fn a_short_major_road_is_kept_by_the_min_size_of_its_rule() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Line),
            rules: vec![RuleDef {
                when: Some("class == 'major'".to_owned()),
                min_size: Some(PixelThreshold::ZERO),
                ..RuleDef::default()
            }],
            ..LayerDef::new("roads", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let major = SourceFeature {
        id: Some(1),
        geometry: Geometry::Line(LineString::from(vec![(0.01, 0.01), (0.010_002, 0.01)])),
        props: vec![(KeyId::new(0), Prop::Str("major".into()))],
    };
    let paths = plan.paths_of(0, &[], Crs::Wgs84, &major).unwrap();
    let taken: Vec<String> = paths.iter().map(ToString::to_string).collect();
    insta::assert_debug_snapshot!(taken, @r#"
    [
        "roads: rule 1 `where: class == 'major'`",
    ]
    "#);
}

#[test]
fn a_short_minor_road_is_smaller_than_the_layer_min_size_at_every_zoom() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Line),
            rules: vec![RuleDef {
                when: Some("class == 'major'".to_owned()),
                min_size: Some(PixelThreshold::ZERO),
                ..RuleDef::default()
            }],
            ..LayerDef::new("roads", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let minor = SourceFeature {
        id: Some(2),
        geometry: Geometry::Line(LineString::from(vec![(0.01, 0.01), (0.010_002, 0.01)])),
        props: vec![(KeyId::new(0), Prop::Str("minor".into()))],
    };
    let paths = plan.paths_of(0, &[], Crs::Wgs84, &minor).unwrap();
    let taken: Vec<String> = paths.iter().map(ToString::to_string).collect();
    insta::assert_debug_snapshot!(taken, @r#"
    [
        "roads: no rule matches",
        "roads: no rule: dropped, smaller than `min_size` at every zoom",
    ]
    "#);
}
