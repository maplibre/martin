//! What `Plan::new` accepts and rejects.

use martin_tilegen::LayerGrid;
use martin_tilegen::plan::{AttributesDef, LayerDef, Plan, PlanError, TableDef};

const GRID: LayerGrid = LayerGrid {
    extent: 4096,
    buffer: 64,
};

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
    assert!(matches!(err, PlanError::TooManyLayers(257)), "{err}");
}

#[test]
fn accepts_256_layers() {
    Plan::new(vec![TableDef {
        columns: vec![],
        dynamic_props: false,
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
        dynamic_props: false,
        layers: vec![LayerDef::new("deep", 0..=28, GRID)],
    }])
    .unwrap_err();
    assert_eq!(err.to_string(), "layer `deep`: zoom 28 is above 27");
}

#[test]
fn unknown_attribute_columns_need_dynamic_props() {
    let table = |dynamic_props| TableDef {
        columns: vec!["name".to_owned()],
        dynamic_props,
        layers: vec![LayerDef {
            attributes: AttributesDef::Columns(vec!["kind".to_owned()]),
            ..LayerDef::new("roads", 0..=14, GRID)
        }],
    };
    let err = Plan::new(vec![table(false)]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "layer `roads`: attribute `kind` is not a table column"
    );
    Plan::new(vec![table(true)]).unwrap();
}

#[test]
fn rejects_a_repeated_attribute_column() {
    let err = Plan::new(vec![TableDef {
        columns: vec!["name".to_owned()],
        dynamic_props: false,
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
