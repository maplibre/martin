//! What `Plan::new` accepts and rejects.

use martin_core::generate::props::Prop;
use martin_tilegen::LayerGrid;
use martin_tilegen::plan::{
    AttributesDef, ComputedAttr, LayerDef, Plan, PlanError, RuleDef, TableDef, ValueDef,
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

#[test]
fn rejects_a_repeated_computed_attribute() {
    let attr = ComputedAttr {
        name: "big".to_owned(),
        value: ValueDef::Literal(Prop::Bool(true)),
        zooms: 0..=30,
    };
    let err = Plan::new(vec![TableDef {
        columns: vec![],
        dynamic_props: false,
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
        columns: vec!["class".to_owned()],
        dynamic_props: false,
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
