use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use geo_types::Coord;
use martin_core::generate::props::{KeyId, Prop};
use martin_core::generate::source::{Geometry, SourceFeature};
use martin_tilegen::plan::{
    AttributesDef, ColumnDef, ComputedAttr, IdDef, LayerDef, Plan, TableDef, ValueDef,
};
use martin_tilegen::{FeatureGeom, LayerGrid};

const COLUMNS: [&str; 6] = [
    "highway",
    "railway",
    "place",
    "population",
    "name",
    "name:en",
];

const POINT: [Coord<f64>; 1] = [Coord { x: 0.0, y: 0.0 }];

fn plan(layer: LayerDef) -> Plan {
    Plan::new(vec![TableDef {
        columns: COLUMNS
            .iter()
            .map(|&name| ColumnDef::Value(name.to_owned()))
            .collect(),
        layers: vec![layer],
    }])
    .expect("valid plan")
}

/// The feature as the source scans it: only the columns the plan keeps, under their scanned keys.
fn feature(plan: &Plan) -> SourceFeature {
    let columns = plan.columns(0);
    let props = [
        ("highway", Prop::Str("primary".to_owned())),
        ("population", Prop::I64(120_000)),
        ("name", Prop::Str("Hauptstraße".to_owned())),
    ]
    .into_iter()
    .filter_map(|(name, value)| {
        let pos = columns.iter().position(|column| column == name)?;
        Some((KeyId::new(u32::try_from(pos).ok()?), value))
    })
    .collect();
    SourceFeature {
        id: None,
        geometry: Geometry::Points(POINT.to_vec()),
        props,
    }
}

fn eval(c: &mut Criterion) {
    let layer = || LayerDef {
        attributes: AttributesDef::None,
        ..LayerDef::new(
            "layer",
            0..=14,
            LayerGrid {
                extent: 4096,
                buffer: 64,
            },
        )
    };
    let plans = [
        (
            "filter",
            plan(LayerDef {
                filter: Some(
                    "highway in ['motorway', 'trunk', 'primary', 'secondary'] || railway == 'rail' \
                     || (place != null && population != null)"
                        .to_owned(),
                ),
                ..layer()
            }),
        ),
        (
            "label",
            plan(LayerDef {
                attributes: AttributesDef::Computed {
                    attributes: vec![ComputedAttr {
                        name: "label".to_owned(),
                        value: ValueDef::Expr("coalesce(feature['name:en'], name)".to_owned()),
                        zooms: 0..=14,
                    }],
                    prefixes: vec![],
                },
                ..layer()
            }),
        ),
        (
            "rank",
            plan(LayerDef {
                id: IdDef::Expr(
                    "population >= 1000000 ? 1 : population >= 100000 ? 2 : population >= 10000 ? 3 : 4"
                        .to_owned(),
                ),
                ..layer()
            }),
        ),
    ];
    let mut group = c.benchmark_group("expr");
    for (name, plan) in &plans {
        let feature = feature(plan);
        let mut evaluator = plan.evaluator();
        group.bench_function(*name, |b| {
            b.iter(|| {
                evaluator
                    .evaluate(0, black_box(&feature), FeatureGeom::Points(&POINT), |f| {
                        black_box(f);
                        Ok(())
                    })
                    .expect("evaluates");
            });
        });
    }
    group.finish();
}

criterion_group!(benches, eval);
criterion_main!(benches);
