fn main() {}
// use std::collections::HashMap;
//
// use criterion::{criterion_group, criterion_main, Criterion};
// use martin::pg::function_source::{FunctionInfo, FunctionSource};
// use martin::pg::table_source::{TableInfo, TableSource};
// use martin::source::{Source, Xyz};
//
// #[path = "../tests/utils.rs"]
// mod utils;
// use utils::*;
//
// async fn mock_table_source(schema: &str, table: &str) -> TableSource {
//     TableSource::new(
//         format!("{schema}.{table}"),
//         TableInfo {
//             schema: schema.to_owned(),
//             table: table.to_owned(),
//             id_column: None,
//             geometry_column: "geom".to_owned(),
//             minzoom: None,
//             maxzoom: None,
//             bounds: None,
//             srid: 3857,
//             extent: Some(4096),
//             buffer: Some(64),
//             clip_geom: Some(true),
//             geometry_type: None,
//             properties: HashMap::new(),
//             unrecognized: HashMap::new(),
//         },
//         mock_pool().await,
//     )
// }
//
// fn mock_function_source(schema: &str, function: &str) -> FunctionSource {
//     // id: format!("{schema}.{function}"),
//     FunctionInfo {
//         schema: schema.to_owned(),
//         function: function.to_owned(),
//         minzoom: None,
//         maxzoom: None,
//         bounds: None,
//         unrecognized: HashMap::new(),
//     }
// }
//
// async fn get_table_source() {
//     let source = mock_table_source("public", "table_source").await;
//     let _tilejson = source.get_tilejson();
// }
//
// async fn get_table_source_tile() {
//     let source = mock_table_source("public", "table_source").await;
//     let xyz = Xyz { z: 0, x: 0, y: 0 };
//     let _tile = source.get_tile(&xyz, &None).await.unwrap();
// }
//
// async fn get_composite_source() {
//     let points1 = mock_table_source("public", "points1");
//     let points2 = mock_table_source("public", "points2");
//
//     let source = CompositeSource {
//         id: "public.points1,public.points2".to_owned(),
//         table_sources: vec![points1, points2],
//     };
//
//     let _tilejson = source.get_tilejson();
// }
//
// async fn get_composite_source_tile() {
//     let points1 = mock_table_source("public", "points1");
//     let points2 = mock_table_source("public", "points2");
//
//     let source = CompositeSource {
//         id: "public.points1,public.points2".to_owned(),
//         table_sources: vec![points1, points2],
//     };
//
//     let xyz = Xyz { z: 0, x: 0, y: 0 };
//     let _tile = source.get_tile(&xyz, &None).await.unwrap();
// }
//
// async fn get_function_source() {
//     let source = mock_function_source("public", "function_zxy_query");
//     let _tilejson = source.get_tilejson();
// }
//
// async fn get_function_source_tile() {
//     let source = mock_function_source("public", "function_zxy_query");
//     let xyz = Xyz { z: 0, x: 0, y: 0 };
//
//     let _tile = source.get_tile(&xyz, &None).await.unwrap();
// }
//
// fn table_source(c: &mut Criterion) {
//     c.bench_function("get_table_source", |b| b.iter(get_table_source));
//     c.bench_function("get_table_source_tile", |b| b.iter(get_table_source_tile));
// }
//
// fn composite_source(c: &mut Criterion) {
//     c.bench_function("get_composite_source", |b| b.iter(get_composite_source));
//     c.bench_function("get_composite_source_tile", |b| {
//         b.iter(get_composite_source_tile);
//     });
// }
//
// fn function_source(c: &mut Criterion) {
//     c.bench_function("get_function_source", |b| b.iter(get_function_source));
//     c.bench_function("get_function_source_tile", |b| {
//         b.iter(get_function_source_tile);
//     });
// }
//
// criterion_group!(benches, table_source, composite_source, function_source);
// criterion_main!(benches);
