use std::collections::HashMap;

use criterion::{criterion_group, criterion_main, Criterion};
use martin::composite_source::CompositeSource;
use martin::dev;
use martin::function_source::FunctionSource;
use martin::source::{Source, Xyz};
use martin::table_source::TableSource;

fn mock_table_source(schema: &str, table: &str) -> TableSource {
    TableSource {
        id: format!("{schema}.{table}"),
        schema: schema.to_owned(),
        table: table.to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        minzoom: None,
        maxzoom: None,
        bounds: None,
        srid: 3857,
        extent: Some(4096),
        buffer: Some(64),
        clip_geom: Some(true),
        geometry_type: None,
        properties: HashMap::new(),
    }
}

fn mock_function_source(schema: &str, function: &str) -> FunctionSource {
    FunctionSource {
        id: format!("{schema}.{function}"),
        schema: schema.to_owned(),
        function: function.to_owned(),
        minzoom: None,
        maxzoom: None,
        bounds: None,
    }
}

async fn get_table_source() {
    let source = mock_table_source("public", "table_source");
    let _tilejson = source.get_tilejson();
}

async fn get_table_source_tile() {
    let pool = dev::make_pool().await;
    let mut connection = pool.get().await.unwrap();

    let source = mock_table_source("public", "table_source");
    let xyz = Xyz { z: 0, x: 0, y: 0 };

    let _tile = source.get_tile(&mut connection, &xyz, &None).await.unwrap();
}

async fn get_composite_source() {
    let points1 = mock_table_source("public", "points1");
    let points2 = mock_table_source("public", "points2");

    let source = CompositeSource {
        id: "public.points1,public.points2".to_owned(),
        table_sources: vec![points1, points2],
    };

    let _tilejson = source.get_tilejson();
}

async fn get_composite_source_tile() {
    let pool = dev::make_pool().await;
    let mut connection = pool.get().await.unwrap();

    let points1 = mock_table_source("public", "points1");
    let points2 = mock_table_source("public", "points2");

    let source = CompositeSource {
        id: "public.points1,public.points2".to_owned(),
        table_sources: vec![points1, points2],
    };

    let xyz = Xyz { z: 0, x: 0, y: 0 };
    let _tile = source.get_tile(&mut connection, &xyz, &None).await.unwrap();
}

async fn get_function_source() {
    let source = mock_function_source("public", "function_source");
    let _tilejson = source.get_tilejson();
}

async fn get_function_source_tile() {
    let pool = dev::make_pool().await;
    let mut connection = pool.get().await.unwrap();

    let source = mock_function_source("public", "function_source");
    let xyz = Xyz { z: 0, x: 0, y: 0 };

    let _tile = source.get_tile(&mut connection, &xyz, &None).await.unwrap();
}

fn table_source(c: &mut Criterion) {
    c.bench_function("get_table_source", |b| b.iter(get_table_source));
    c.bench_function("get_table_source_tile", |b| b.iter(get_table_source_tile));
}

fn composite_source(c: &mut Criterion) {
    c.bench_function("get_composite_source", |b| b.iter(get_composite_source));
    c.bench_function("get_composite_source_tile", |b| {
        b.iter(get_composite_source_tile)
    });
}

fn function_source(c: &mut Criterion) {
    c.bench_function("get_function_source", |b| b.iter(get_function_source));
    c.bench_function("get_function_source_tile", |b| {
        b.iter(get_function_source_tile)
    });
}

criterion_group!(benches, table_source, composite_source, function_source);
criterion_main!(benches);
