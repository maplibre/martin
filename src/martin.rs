use actix_web::*;
use actix::*;
use futures::future::Future;
use mapbox_expressions_to_sql;
use std::cell::RefCell;
use std::rc::Rc;
use tilejson::TileJSONBuilder;

use super::messages;
use super::db::DbExecutor;
use super::source::Sources;
use super::worker_actor::WorkerActor;
use super::coordinator_actor::CoordinatorActor;

pub struct State {
    db: Addr<Syn, DbExecutor>,
    sources: Rc<RefCell<Sources>>,
    coordinator_addr: Addr<Syn, CoordinatorActor>,
}

fn index(req: HttpRequest<State>) -> Box<Future<Item = HttpResponse, Error = Error>> {
    req.state()
        .db
        .send(messages::GetSources {})
        .from_err()
        .and_then(move |res| match res {
            Ok(sources) => {
                let coordinator_addr = &req.state().coordinator_addr;
                coordinator_addr.do_send(messages::RefreshSources {
                    sources: sources.clone(),
                });
                Ok(httpcodes::HTTPOk.build().json(sources)?)
            }
            Err(_) => Ok(httpcodes::HTTPInternalServerError.into()),
        })
        .responder()
}

fn source(req: HttpRequest<State>) -> Result<HttpResponse> {
    let source_ids = req.match_info()
        .get("sources")
        .ok_or(error::ErrorBadRequest("invalid source"))?;

    let mut tilejson_builder = TileJSONBuilder::new();
    tilejson_builder.scheme("tms");
    tilejson_builder.name(&source_ids);

    let tiles_url = format!(
        "{}/{{z}}/{{x}}/{{y}}.pbf",
        req.url_for("tilejson", &[source_ids]).unwrap()
    );

    tilejson_builder.tiles(vec![&tiles_url]);

    let tilejson = tilejson_builder.finalize();
    Ok(httpcodes::HTTPOk.build().json(tilejson)?)
}

fn tile(req: HttpRequest<State>) -> Result<Box<Future<Item = HttpResponse, Error = Error>>> {
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
                    .body(tile)?),
                _ => Ok(HttpResponse::Ok()
                    .content_type("application/x-protobuf")
                    .body(tile)?),
            },
            Err(_) => Ok(httpcodes::HTTPInternalServerError.into()),
        })
        .responder())
}

pub fn new(
    db_sync_arbiter: Addr<Syn, DbExecutor>,
    coordinator_addr: Addr<Syn, CoordinatorActor>,
    sources: Sources,
) -> Application<State> {
    let sources_rc = Rc::new(RefCell::new(sources));

    let worker_actor = WorkerActor {
        sources: sources_rc.clone(),
    };

    let worker_addr: Addr<Syn, _> = worker_actor.start();
    coordinator_addr.do_send(messages::Connect { addr: worker_addr });

    let state = State {
        db: db_sync_arbiter,
        sources: sources_rc.clone(),
        coordinator_addr: coordinator_addr,
    };

    let cors = middleware::cors::Cors::build()
        .finish()
        .expect("Can not create CORS middleware");

    Application::with_state(state)
        .middleware(middleware::Logger::default())
        .middleware(cors)
        .resource("/index.json", |r| r.method(Method::GET).a(index))
        .resource("/{sources}.json", |r| {
            r.name("tilejson");
            r.method(Method::GET).f(source)
        })
        .resource("/{sources}/{z}/{x}/{y}.pbf", |r| {
            r.method(Method::GET).f(tile)
        })
}
