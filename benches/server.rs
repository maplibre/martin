use criterion::{criterion_group, criterion_main, Criterion};

use actix_web::dev::Service;
use actix_web::{test, App};

use martin::dev::{mock_function_sources, mock_state, mock_table_sources};
use martin::server::router;

fn criterion_benchmark(c: &mut Criterion) {
  let state = test::run_on(|| mock_state(mock_table_sources(), mock_function_sources()));
  let mut app = test::init_service(App::new().app_data(state).configure(router));

  c.bench_function("/public.table_source/0/0/0.pbf", |b| {
    b.iter(|| {
      let req = test::TestRequest::get()
        .uri("/public.table_source/0/0/0.pbf")
        .to_request();

      let future = test::run_on(|| app.call(req));
      let _response = test::block_on(future).unwrap();
    })
  });

  c.bench_function("/rpc/public.function_source/0/0/0.pbf", |b| {
    b.iter(|| {
      let req = test::TestRequest::get()
        .uri("/rpc/public.function_source/0/0/0.pbf")
        .to_request();

      let future = test::run_on(|| app.call(req));
      let _response = test::block_on(future).unwrap();
    })
  });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
