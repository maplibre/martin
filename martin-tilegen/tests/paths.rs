//! Which paths through the layers a feature takes, as the plan evaluates it.

use geo_types::{Coord, LineString};
use martin_tilegen::plan::{
    AttributesDef, ColumnDef, ColumnValues, ComputedAttr, GeometryType, LayerDef, Literal, Plan, RuleDef, TableDef,
    ValueDef,
};
use martin_tilegen::props::{KeyId, Prop};
use martin_tilegen::source::{Crs, Geometry, SourceFeature};
use martin_tilegen::{LayerGrid, PixelThreshold};

const GRID: LayerGrid = LayerGrid {
    extent: 4096,
    buffer: 64,
};

#[test]
fn a_feature_takes_the_first_rule_it_matches() {
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
    let point = Geometry::Point(Coord { x: 0.01, y: 0.01 });
    let city = SourceFeature {
        id: Some(1),
        geometry: point.clone(),
        props: vec![(KeyId::new(0), Prop::Str("city".into()))],
    };
    let capital = SourceFeature {
        id: Some(2),
        geometry: point.clone(),
        props: vec![(KeyId::new(0), Prop::Str("capital".into()))],
    };
    let unnamed = SourceFeature {
        id: Some(3),
        geometry: point,
        props: vec![],
    };
    let taken: Vec<Vec<String>> = [city, capital, unnamed]
        .iter()
        .map(|f| {
            let paths = plan.paths_of(0, &[], Crs::Wgs84, f).unwrap();
            paths.iter().map(ToString::to_string).collect()
        })
        .collect();
    insta::assert_debug_snapshot!(taken, @r#"
    [
        [
            "places: rule 1 `class == 'city'`",
        ],
        [
            "places: rule 2 `class.startsWith('c')`",
        ],
        [
            "places: no rule",
        ],
    ]
    "#);
}

#[test]
fn a_feature_is_where_not_where_or_null() {
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("population".to_owned(), None)],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Point),
            filter: Some("population > 1000".to_owned()),
            ..LayerDef::new("places", 0..=14, GRID)
        }],
    }])
    .unwrap();
    let point = Geometry::Point(Coord { x: 0.01, y: 0.01 });
    let big = SourceFeature {
        id: Some(1),
        geometry: point.clone(),
        props: vec![(KeyId::new(0), Prop::I64(5000))],
    };
    let small = SourceFeature {
        id: Some(2),
        geometry: point.clone(),
        props: vec![(KeyId::new(0), Prop::I64(10))],
    };
    let unknown = SourceFeature {
        id: Some(3),
        geometry: point,
        props: vec![],
    };
    let taken: Vec<Vec<String>> = [big, small, unknown]
        .iter()
        .map(|f| {
            let paths = plan.paths_of(0, &[], Crs::Wgs84, f).unwrap();
            paths.iter().map(ToString::to_string).collect()
        })
        .collect();
    insta::assert_debug_snapshot!(taken, @r#"
    [
        [
            "places: where `population > 1000`",
        ],
        [
            "places: not where `population > 1000`",
        ],
        [
            "places: where `population > 1000` with `population` null",
        ],
    ]
    "#);
}

#[test]
fn only_the_features_of_a_rule_take_the_choices_of_its_attributes() {
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
    let point = Geometry::Point(Coord { x: 0.01, y: 0.01 });
    let city = SourceFeature {
        id: Some(1),
        geometry: point.clone(),
        props: vec![
            (KeyId::new(0), Prop::Str("city".into())),
            (KeyId::new(1), Prop::Str("Wien".into())),
        ],
    };
    let town = SourceFeature {
        id: Some(2),
        geometry: point,
        props: vec![
            (KeyId::new(0), Prop::Str("town".into())),
            (KeyId::new(1), Prop::Str("Baden".into())),
        ],
    };
    let taken: Vec<Vec<String>> = [city, town]
        .iter()
        .map(|f| {
            let paths = plan.paths_of(0, &[], Crs::Wgs84, f).unwrap();
            paths.iter().map(ToString::to_string).collect()
        })
        .collect();
    insta::assert_debug_snapshot!(taken, @r#"
    [
        [
            "places: rule 1 `class == 'city'`",
            "places: `coalesce(name_en, name)`: `name` is the first not null",
        ],
        [
            "places: no rule",
        ],
    ]
    "#);
}

#[test]
fn a_feature_takes_the_path_of_its_known_value() {
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
        "devices: every feature",
        "devices: `type` is 'stop'",
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
    let short = Geometry::Line(LineString::from(vec![(0.01, 0.01), (0.010_002, 0.01)]));
    let major = SourceFeature {
        id: Some(1),
        geometry: short.clone(),
        props: vec![(KeyId::new(0), Prop::Str("major".into()))],
    };
    let minor = SourceFeature {
        id: Some(2),
        geometry: short,
        props: vec![(KeyId::new(0), Prop::Str("minor".into()))],
    };
    let branches: Vec<String> = plan.branches(0, &[]).unwrap().iter().map(ToString::to_string).collect();
    let taken: Vec<Vec<String>> = [major, minor]
        .iter()
        .map(|f| {
            let paths = plan.paths_of(0, &[], Crs::Wgs84, f).unwrap();
            paths.iter().map(ToString::to_string).collect()
        })
        .collect();
    insta::assert_debug_snapshot!((branches, taken), @r#"
    (
        [
            "roads: rule 1 `class == 'major'`",
            "roads: no rule",
            "roads: no rule: kept only at z14 by `min_size_at_maxzoom`",
            "roads: no rule: smaller than `min_size` at every zoom",
        ],
        [
            [
                "roads: rule 1 `class == 'major'`",
            ],
            [
                "roads: no rule",
                "roads: no rule: smaller than `min_size` at every zoom",
            ],
        ],
    )
    "#);
}
