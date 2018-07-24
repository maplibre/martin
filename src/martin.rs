use actix::*;
use actix_web::*;
use futures::future::Future;
use mapbox_expressions_to_sql;
use std::cell::RefCell;
use std::rc::Rc;
use tilejson::TileJSONBuilder;

use super::coordinator_actor::CoordinatorActor;
use super::db::DbExecutor;
use super::messages;
use super::source::Sources;
use super::worker_actor::WorkerActor;

pub struct State {
    db: Addr<DbExecutor>,
    sources: Rc<RefCell<Sources>>,
    coordinator_addr: Addr<CoordinatorActor>,
}

fn index(req: &HttpRequest<State>) -> Box<Future<Item = HttpResponse, Error = Error>> {
    let coordinator_addr = req.state().coordinator_addr.clone();

    req.state()
        .db
        .send(messages::GetSources {})
        .from_err()
        .and_then(move |res| match res {
            Ok(sources) => {
                coordinator_addr.do_send(messages::RefreshSources {
                    sources: sources.clone(),
                });

                Ok(HttpResponse::Ok()
                    .header("Access-Control-Allow-Origin", "*")
                    .json(sources))
            }
            Err(_) => Ok(HttpResponse::InternalServerError().into()),
        })
        .responder()
}

fn source(req: &HttpRequest<State>) -> Result<HttpResponse> {
    let source_ids = req.match_info()
        .get("sources")
        .ok_or(error::ErrorBadRequest("invalid source"))?;

    let path = req.headers()
        .get("x-rewrite-url")
        .map_or(String::from(source_ids), |header| {
            let parts: Vec<&str> = header.to_str().unwrap().split(".").collect();
            let (_, parts_without_extension) = parts.split_last().unwrap();
            let path_without_extension = parts_without_extension.join(".");
            let (_, path_without_leading_slash) = path_without_extension.split_at(1);

            String::from(path_without_leading_slash)
        });

    let conn = req.connection_info();
    let tiles_url = format!(
        "{}://{}/{}/{{z}}/{{x}}/{{y}}.pbf",
        conn.scheme(),
        conn.host(),
        path
    );

    let mut tilejson_builder = TileJSONBuilder::new();
    tilejson_builder.scheme("tms");
    tilejson_builder.name(&source_ids);
    tilejson_builder.tiles(vec![&tiles_url]);
    let tilejson = tilejson_builder.finalize();

    Ok(HttpResponse::Ok()
        .header("Access-Control-Allow-Origin", "*")
        .json(tilejson))
}

fn tile(req: &HttpRequest<State>) -> Result<Box<Future<Item = HttpResponse, Error = Error>>> {
    let sources = &req.state().sources.borrow();

    let source_id = req.match_info()
        .get("sources")
        .ok_or(error::ErrorBadRequest("invalid source"))?;

    let source = sources.get(source_id).ok_or(error::ErrorNotFound(format!(
        "source {} not found",
        source_id
    )))?;

    let z = req.match_info()
        .get("z")
        .and_then(|i| i.parse::<u32>().ok())
        .ok_or(error::ErrorBadRequest("invalid z"))?;

    let x = req.match_info()
        .get("x")
        .and_then(|i| i.parse::<u32>().ok())
        .ok_or(error::ErrorBadRequest("invalid x"))?;

    let y = req.match_info()
        .get("y")
        .and_then(|i| i.parse::<u32>().ok())
        .ok_or(error::ErrorBadRequest("invalid y"))?;

    let condition = req.query()
        .get("filter")
        .and_then(|filter| mapbox_expressions_to_sql::parse(filter).ok());

    Ok(req.state()
        .db
        .send(messages::GetTile {
            z: z,
            x: x,
            y: y,
            source: source.clone(),
            condition: condition,
        })
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
        .responder())
}

pub fn new(
    db_sync_arbiter: Addr<DbExecutor>,
    coordinator_addr: Addr<CoordinatorActor>,
    sources: Sources,
) -> App<State> {
    let sources_rc = Rc::new(RefCell::new(sources));

    let worker_actor = WorkerActor {
        sources: sources_rc.clone(),
    };

    let worker_addr: Addr<_> = worker_actor.start();
    coordinator_addr.do_send(messages::Connect { addr: worker_addr });

    let state = State {
        db: db_sync_arbiter,
        sources: sources_rc.clone(),
        coordinator_addr: coordinator_addr,
    };

    App::with_state(state)
        .middleware(middleware::Logger::default())
        .resource("/index.json", |r| r.method(http::Method::GET).a(index))
        .resource("/{sources}.json", |r| {
            r.name("tilejson");
            r.method(http::Method::GET).f(source)
        })
        .resource("/{sources}/{z}/{x}/{y}.pbf", |r| {
            r.method(http::Method::GET).f(tile)
        })
}
