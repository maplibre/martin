use std::cell::RefCell;
use std::rc::Rc;

use actix::{Actor, Addr, SyncArbiter, SystemRunner};

use actix_web::{
    error, http, middleware, web, App, Either, Error, HttpResponse, HttpServer, Result,
};

use futures::Future;

use super::config::Config;
use super::coordinator_actor::CoordinatorActor;
use super::db::PostgresPool;
use super::db_actor::DBActor;
use super::function_source::FunctionSources;
use super::messages;
use super::table_source::TableSources;
use super::worker_actor::WorkerActor;

struct State {
    db: Addr<DBActor>,
    coordinator: Addr<CoordinatorActor>,
    table_sources: Rc<RefCell<Option<TableSources>>>,
    function_sources: Rc<RefCell<Option<FunctionSources>>>,
    watch_mode: bool,
}

type SourcesResult = Either<HttpResponse, Box<dyn Future<Item = HttpResponse, Error = Error>>>;

fn get_table_sources(state: web::Data<State>) -> SourcesResult {
    if !state.watch_mode {
        let table_sources = state.table_sources.borrow().clone();
        let response = HttpResponse::Ok().json(table_sources);
        return Either::A(response);
    }

    info!("Scanning database for table sources");
    let response = state
        .db
        .send(messages::GetTableSources {})
        .from_err()
        .and_then(move |table_sources| match table_sources {
            Ok(table_sources) => {
                state.coordinator.do_send(messages::RefreshTableSources {
                    table_sources: Some(table_sources.clone()),
                });

                Ok(HttpResponse::Ok().json(table_sources))
            }
            Err(_) => Ok(HttpResponse::InternalServerError().into()),
        });

    return Either::B(Box::new(response));
}

#[derive(Deserialize)]
struct SourceRequest {
    source_id: String,
}

fn get_table_source(
    path: web::Path<SourceRequest>,
    state: web::Data<State>,
) -> Result<HttpResponse> {
    let response = format!("table source {}", path.source_id);

    let table_sources = state
        .table_sources
        .borrow()
        .clone()
        .ok_or_else(|| error::ErrorNotFound("There is no table sources"))?;

    let source = table_sources.get(&path.source_id).ok_or_else(|| {
        error::ErrorNotFound(format!("Table source '{}' not found", path.source_id))
    })?;

    Ok(HttpResponse::Ok().json("{}"))
}

pub fn router(cfg: &mut web::ServiceConfig) {
    cfg.route("/index.json", web::get().to(get_table_sources))
        .route("/{source_id}.json", web::get().to(get_table_sources));
}

pub fn new(pool: PostgresPool, config: Config, watch_mode: bool) -> SystemRunner {
    let sys = actix_rt::System::new("server");

    let db = SyncArbiter::start(3, move || DBActor(pool.clone()));
    let coordinator: Addr<_> = CoordinatorActor::default().start();

    let keep_alive = config.keep_alive;
    let worker_processes = config.worker_processes;
    let listen_addresses = config.listen_addresses.clone();

    HttpServer::new(move || {
        let table_sources = Rc::new(RefCell::new(config.table_sources.clone()));
        let function_sources = Rc::new(RefCell::new(config.function_sources.clone()));

        let worker_actor = WorkerActor {
            table_sources: table_sources.clone(),
            function_sources: function_sources.clone(),
        };

        let worker: Addr<_> = worker_actor.start();
        coordinator.do_send(messages::Connect { addr: worker });

        let state = State {
            db: db.clone(),
            coordinator: coordinator.clone(),
            table_sources,
            function_sources,
            watch_mode,
        };

        let cors_middleware = middleware::DefaultHeaders::new()
            .header(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");

        App::new()
            .data(state)
            .wrap(cors_middleware)
            .wrap(middleware::NormalizePath)
            .wrap(middleware::Logger::default())
            .wrap(middleware::Compress::default())
            .configure(router)
    })
    .bind(listen_addresses.clone())
    .unwrap_or_else(|_| panic!("Can't bind to {}", listen_addresses))
    .keep_alive(keep_alive)
    .shutdown_timeout(0)
    .workers(worker_processes)
    .start();

    sys
}
