use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use martin_tilegen::expr::{CompiledExpr, ExprKeys, PropSlots};
use martin_tilegen::props::{KeyId, Prop};

const COLUMNS: [&str; 6] = [
    "highway",
    "railway",
    "place",
    "population",
    "name",
    "name:en",
];

fn eval(c: &mut Criterion) {
    let keys = ExprKeys::columns(&COLUMNS);
    let props = vec![
        (KeyId::from(0), Prop::Str("primary".to_owned())),
        (KeyId::from(3), Prop::I64(120_000)),
        (KeyId::from(4), Prop::Str("Hauptstraße".to_owned())),
    ];
    let mut group = c.benchmark_group("expr");
    for (name, source) in [
        (
            "filter",
            "highway in ['motorway', 'trunk', 'primary', 'secondary'] || railway == 'rail' \
             || (place != null && population != null)",
        ),
        ("label", "coalesce(feature['name:en'], name)"),
        (
            "rank",
            "population >= 1000000 ? 1 : population >= 100000 ? 2 : population >= 10000 ? 3 : 4",
        ),
    ] {
        let expr = CompiledExpr::compile(source, &COLUMNS, false).expect("valid expression");
        let mut slots = PropSlots::default();
        group.bench_function(name, |b| {
            b.iter(|| {
                let view = slots.bind(&keys, black_box(&props));
                black_box(expr.eval(&view).is_ok())
            });
        });
    }
    group.finish();
}

criterion_group!(benches, eval);
criterion_main!(benches);
