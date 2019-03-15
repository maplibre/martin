use actix::*;
use actix_web::*;
use futures::future::Future;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::coordinator_actor::CoordinatorActor;
use super::db_executor::DbExecutor;
use super::function_source::FunctionSources;
use super::messages;
use super::table_source::TableSources;
use super::utils::{build_tilejson, parse_xyz};
use super::worker_actor::WorkerActor;

pub type Query = HashMap<String, String>;

pub struct State {
    db: Addr<DbExecutor>,
    coordinator: Addr<CoordinatorActor>,
    table_sources: Rc<RefCell<Option<TableSources>>>,
    function_sources: Rc<RefCell<Option<FunctionSources>>>,
}

fn get_table_sources(
    req: &HttpRequest<State>,
) -> Result<Box<Future<Item = HttpResponse, Error = Error>>> {
    let state = &req.state();
    let coordinator = state.coordinator.clone();

    let result = req.state().db.send(messages::GetTableSources {});

    let response = result
        .from_err()
        .and_then(move |res| match res {
            Ok(table_sources) => {
                coordinator.do_send(messages::RefreshTableSources {
                    table_sources: Some(table_sources.clone()),
                });

                Ok(HttpResponse::Ok()
                    .header("Access-Control-Allow-Origin", "*")
                    .json(table_sources))
            }
            Err(_) => Ok(HttpResponse::InternalServerError().into()),
        })
        .responder();

    Ok(response)
}

fn get_table_source(req: &HttpRequest<State>) -> Result<HttpResponse> {
    let state = &req.state();

    let table_sources = state
        .table_sources
        .borrow()
        .clone()
        .ok_or_else(|| error::ErrorNotFound("There is no table sources"))?;

    let params = req.match_info();
    let source_id = params
        .get("source_id")
        .ok_or_else(|| error::ErrorBadRequest("Invalid table source id"))?;

    let source = table_sources
        .get(source_id)
        .ok_or_else(|| error::ErrorNotFound(format!("Table source '{}' not found", source_id)))?;

    let tilejson = build_tilejson(
        source.clone(),
        &req.connection_info(),
        req.path(),
        req.query_string(),
        req.headers(),
    )
    .map_err(|e| error::ErrorBadRequest(format!("Can't build TileJSON: {}", e)))?;

    Ok(HttpResponse::Ok()
        .header("Access-Control-Allow-Origin", "*")
        .json(tilejson))
}

fn get_table_source_tile(
    req: &HttpRequest<State>,
) -> Result<Box<Future<Item = HttpResponse, Error = Error>>> {
    let state = &req.state();

    let table_sources = state
        .table_sources
        .borrow()
        .clone()
        .ok_or_else(|| error::ErrorNotFound("There is no table sources"))?;

    let params = req.match_info();
    let source_id = params
        .get("source_id")
        .ok_or_else(|| error::ErrorBadRequest("Invalid table source id"))?;

    let source = table_sources
        .get(source_id)
        .ok_or_else(|| error::ErrorNotFound(format!("Table source '{}' not found", source_id)))?;

    let xyz = parse_xyz(params)
        .map_err(|e| error::ErrorBadRequest(format!("Can't parse XYZ scheme: {}", e)))?;

    let query = req.query();

    let message = messages::GetTile {
        xyz,
        query: query.clone(),
        source: source.clone(),
    };

    let result = req.state().db.send(message);

    let response = result
        .from_err()
        .and_then(|res| match res {
            Ok(tile) => match tile.len() {
                0 => Ok(HttpResponse::NoContent()
                    .content_type("application/x-protobuf")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(tile)),
                _ => Ok(HttpResponse::Ok()
                    .content_type("application/x-protobuf")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(tile)),
            },
            Err(_) => Ok(HttpResponse::InternalServerError().into()),
        })
        .responder();

    Ok(response)
}

fn get_function_sources(
    req: &HttpRequest<State>,
) -> Result<Box<Future<Item = HttpResponse, Error = Error>>> {
    let state = &req.state();
    let coordinator = state.coordinator.clone();

    let result = req.state().db.send(messages::GetFunctionSources {});
    let response = result
        .from_err()
        .and_then(move |res| match res {
            Ok(function_sources) => {
                coordinator.do_send(messages::RefreshFunctionSources {
                    function_sources: Some(function_sources.clone()),
                });

                Ok(HttpResponse::Ok()
                    .header("Access-Control-Allow-Origin", "*")
                    .json(function_sources))
            }
            Err(_) => Ok(HttpResponse::InternalServerError().into()),
        })
        .responder();

    Ok(response)
}

fn get_function_source(req: &HttpRequest<State>) -> Result<HttpResponse> {
    let state = &req.state();
    let function_sources = state
        .function_sources
        .borrow()
        .clone()
        .ok_or_else(|| error::ErrorNotFound("There is no function sources"))?;

    let params = req.match_info();
    let source_id = params
        .get("source_id")
        .ok_or_else(|| error::ErrorBadRequest("Invalid function source id"))?;

    let source = function_sources.get(source_id).ok_or_else(|| {
        error::ErrorNotFound(format!("Function source '{}' not found", source_id))
    })?;

    let tilejson = build_tilejson(
        source.clone(),
        &req.connection_info(),
        req.path(),
        req.query_string(),
        req.headers(),
    )
    .map_err(|e| error::ErrorBadRequest(format!("Can't build TileJSON: {}", e)))?;

    Ok(HttpResponse::Ok()
        .header("Access-Control-Allow-Origin", "*")
        .json(tilejson))
}

fn get_function_source_tile(
    req: &HttpRequest<State>,
) -> Result<Box<Future<Item = HttpResponse, Error = Error>>> {
    let state = &req.state();
    let function_sources = state
        .function_sources
        .borrow()
        .clone()
        .ok_or_else(|| error::ErrorNotFound("There is no function sources"))?;

    let params = req.match_info();
    let source_id = params
        .get("source_id")
        .ok_or_else(|| error::ErrorBadRequest("Invalid function source id"))?;

    let source = function_sources.get(source_id).ok_or_else(|| {
        error::ErrorNotFound(format!("Function source '{}' not found", source_id))
    })?;

    let xyz = parse_xyz(params)
        .map_err(|e| error::ErrorBadRequest(format!("Can't parse XYZ scheme: {}", e)))?;

    let query = req.query();

    let message = messages::GetTile {
        xyz,
        query: query.clone(),
        source: source.clone(),
    };

    let result = req.state().db.send(message);

    let response = result
        .from_err()
        .and_then(|res| match res {
            Ok(tile) => match tile.len() {
                0 => Ok(HttpResponse::NoContent()
                    .content_type("application/x-protobuf")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(tile)),
                _ => Ok(HttpResponse::Ok()
                    .content_type("application/x-protobuf")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(tile)),
            },
            Err(_) => Ok(HttpResponse::InternalServerError().into()),
        })
        .responder();

    Ok(response)
}

pub fn new(
    db: Addr<DbExecutor>,
    coordinator: Addr<CoordinatorActor>,
    table_sources: Option<TableSources>,
    function_sources: Option<FunctionSources>,
) -> App<State> {
    let table_sources_rc = Rc::new(RefCell::new(table_sources));
    let function_sources_rc = Rc::new(RefCell::new(function_sources));

    let worker_actor = WorkerActor {
        table_sources: table_sources_rc.clone(),
        function_sources: function_sources_rc.clone(),
    };

    let worker: Addr<_> = worker_actor.start();
    coordinator.do_send(messages::Connect { addr: worker });

    let state = State {
        db,
        coordinator,
        table_sources: table_sources_rc.clone(),
        function_sources: function_sources_rc.clone(),
    };

    App::with_state(state)
        .middleware(middleware::Logger::default())
        .resource("/index.json", |r| {
            r.method(http::Method::GET).f(get_table_sources)
        })
        .resource("/{source_id}.json", |r| {
            r.method(http::Method::GET).f(get_table_source)
        })
        .resource("/{source_id}/{z}/{x}/{y}.pbf", |r| {
            r.method(http::Method::GET).f(get_table_source_tile)
        })
        .resource("/rpc/index.json", |r| {
            r.method(http::Method::GET).f(get_function_sources)
        })
        .resource("/rpc/{source_id}.json", |r| {
            r.method(http::Method::GET).f(get_function_source)
        })
        .resource("/rpc/{source_id}/{z}/{x}/{y}.pbf", |r| {
            r.method(http::Method::GET).f(get_function_source_tile)
        })
}

#[cfg(test)]
mod tests {
    extern crate env_logger;

    use super::super::db::setup_connection_pool;
    use super::super::db_executor::DbExecutor;
    use super::super::function_source::{FunctionSource, FunctionSources};
    use super::super::table_source::{TableSource, TableSources};
    use super::*;
    use actix::SyncArbiter;
    use actix_web::{http, test};
    use std::env;

    // TODO: rewrite using test::TestServer::with_factory
    fn build_test_server(
        table_sources: Option<TableSources>,
        function_sources: Option<FunctionSources>,
    ) -> test::TestServer {
        test::TestServer::build_with_state(move || {
            let conn_string: String = env::var("DATABASE_URL").unwrap();
            let pool = setup_connection_pool(&conn_string, None).unwrap();
            let db = SyncArbiter::start(3, move || DbExecutor(pool.clone()));

            let table_sources_rc = Rc::new(RefCell::new(table_sources.clone()));
            let function_sources_rc = Rc::new(RefCell::new(function_sources.clone()));

            let coordinator: Addr<_> = CoordinatorActor::default().start();

            let worker_actor = WorkerActor {
                table_sources: table_sources_rc.clone(),
                function_sources: function_sources_rc.clone(),
            };

            let worker: Addr<_> = worker_actor.start();
            coordinator.do_send(messages::Connect { addr: worker });

            State {
                db,
                coordinator,
                table_sources: table_sources_rc.clone(),
                function_sources: function_sources_rc.clone(),
            }
        })
        .start(|app| {
            app.resource("/index.json", |r| {
                r.method(http::Method::GET).f(get_table_sources)
            })
            .resource("/{source_id}.json", |r| {
                r.method(http::Method::GET).f(get_table_source)
            })
            .resource("/{source_id}/{z}/{x}/{y}.pbf", |r| {
                r.method(http::Method::GET).f(get_table_source_tile)
            })
            .resource("/rpc/index.json", |r| {
                r.method(http::Method::GET).f(get_function_sources)
            })
            .resource("/rpc/{source_id}.json", |r| {
                r.method(http::Method::GET).f(get_function_source)
            })
            .resource("/rpc/{source_id}/{z}/{x}/{y}.pbf", |r| {
                r.method(http::Method::GET).f(get_function_source_tile)
            });
        })
    }

    #[test]
    fn sources_not_found_test() {
        let mut srv = build_test_server(None, None);

        // let request = srv
        //     .client(http::Method::GET, "/index.json")
        //     .finish()
        //     .unwrap();

        // let response = srv.execute(request.send()).unwrap();
        // assert_eq!(response.status().as_u16(), 404);

        let request = srv
            .client(http::Method::GET, "/public.non_existant.json")
            .finish()
            .unwrap();

        let response = srv.execute(request.send()).unwrap();
        assert_eq!(response.status().as_u16(), 404);

        // let request = srv
        //     .client(http::Method::GET, "/rpc/index.json")
        //     .finish()
        //     .unwrap();

        // let response = srv.execute(request.send()).unwrap();
        // assert_eq!(response.status().as_u16(), 404);

        let request = srv
            .client(http::Method::GET, "/rpc/public.non_existant.json")
            .finish()
            .unwrap();

        let response = srv.execute(request.send()).unwrap();
        assert_eq!(response.status().as_u16(), 404);
    }

    #[test]
    fn table_sources_test() {
        let id = "public.table_source";
        let source = TableSource {
            id: id.to_owned(),
            schema: "public".to_owned(),
            table: "table_source".to_owned(),
            id_column: None,
            geometry_column: "geom".to_owned(),
            srid: 3857,
            extent: Some(4096),
            buffer: Some(64),
            clip_geom: Some(true),
            geometry_type: None,
            properties: HashMap::new(),
        };

        let mut table_sources: TableSources = HashMap::new();
        table_sources.insert(id.to_owned(), Box::new(source));

        let mut srv = build_test_server(Some(table_sources), None);

        let request = srv
            .client(http::Method::GET, "/public.table_source.json")
            .finish()
            .unwrap();

        let response = srv.execute(request.send()).unwrap();
        println!("response {:?}", response);
        assert!(response.status().is_success());

        let request = srv
            .client(http::Method::GET, "/public.table_source/0/0/0.pbf")
            .finish()
            .unwrap();

        let response = srv.execute(request.send()).unwrap();
        println!("response {:?}", response);
        assert!(response.status().is_success());
    }

    #[test]
    fn function_sources_test() {
        let id = "public.function_source";
        let source = FunctionSource {
            id: id.to_owned(),
            schema: "public".to_owned(),
            function: "function_source".to_owned(),
        };

        let mut function_sources: FunctionSources = HashMap::new();
        function_sources.insert(id.to_owned(), Box::new(source));

        let mut srv = build_test_server(None, Some(function_sources));

        let request = srv
            .client(http::Method::GET, "/rpc/public.function_source.json")
            .finish()
            .unwrap();

        let response = srv.execute(request.send()).unwrap();
        println!("response {:?}", response);
        assert!(response.status().is_success());

        let request = srv
            .client(http::Method::GET, "/rpc/public.function_source/0/0/0.pbf")
            .finish()
            .unwrap();

        let response = srv.execute(request.send()).unwrap();
        println!("response {:?}", response);
        assert!(response.status().is_success());
    }
}
