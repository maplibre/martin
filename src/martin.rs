use actix_web::{middleware, Application, Error, HttpRequest, HttpResponse, Method, Result};
use actix_web::error::ErrorNotFound;
use actix_web::httpcodes::HTTPOk;
use actix::{Addr, Syn};
use futures::future::Future;

use super::db::{DbExecutor, GetTile};
use super::source::Sources;

pub struct State {
    db: Addr<Syn, DbExecutor>,
    sources: Sources,
}

fn index(req: HttpRequest<State>) -> Result<HttpResponse> {
    let sources = &req.state().sources;
    Ok(HTTPOk.build().json(sources)?)
}

fn source(req: HttpRequest<State>) -> Result<HttpResponse> {
    let sources = &req.state().sources;
    let source_id = req.match_info().get("source").unwrap();

    let source = sources
        .get(source_id)
        .ok_or(ErrorNotFound(format!("source {} not found", source_id)))?;

    Ok(HTTPOk.build().json(source)?)
}

// fn tile(req: HttpRequest<State>) -> Result<HttpResponse> {
fn tile(req: HttpRequest<State>) -> Box<Future<Item = HttpResponse, Error = Error>> {
    let sources = &req.state().sources;
    let source_id = req.match_info().get("source").unwrap();

    let source = sources
        .get(source_id)
        .ok_or(ErrorNotFound(format!("source {} not found", source_id)))?;

    let z = req.match_info()
        .get("z")
        .and_then(|i| i.parse::<u32>().ok())
        .ok_or(ErrorNotFound("invalid z"))?;

    let x = req.match_info()
        .get("x")
        .and_then(|i| i.parse::<u32>().ok())
        .ok_or(ErrorNotFound("invalid x"))?;

    let y = req.match_info()
        .get("y")
        .and_then(|i| i.parse::<u32>().ok())
        .ok_or(ErrorNotFound("invalid y"))?;

    // req.state()
    //     .db
    //     .send(GetTile { z: z, x: x, y: y })
    //     .from_err()
    //     .and_then(|_| {
    //         let response = HttpResponse::Ok()
    //             .content_type("plain/text")
    //             .body(format!("requested tile {} {} {}", z, x, y));

    //         Ok(response?)
    //     })
    //     .responder()

    let res = req.state()
        .db
        .send(GetTile { z: z, x: x, y: y })
        .from_err()
        .and_then(|_| {
            let response = HttpResponse::Ok()
                .content_type("plain/text")
                .body(format!("requested tile {} {} {}", z, x, y));

            Ok(response?)
        })
        .responder();

    res
}

pub fn new(db_sync_arbiter: Addr<Syn, DbExecutor>, sources: Sources) -> Application<State> {
    let state = State {
        db: db_sync_arbiter,
        sources: sources,
    };

    let cors = middleware::cors::Cors::build()
        .finish()
        .expect("Can not create CORS middleware");

    Application::with_state(state)
        .middleware(middleware::Logger::default())
        .middleware(cors)
        .resource("/index.json", |r| r.method(Method::GET).f(index))
        .resource("/{source}.json", |r| r.method(Method::GET).f(source))
        .resource("/{source}/{z}/{x}/{y}.pbf", |r| {
            r.method(Method::GET).a(tile)
        })
}
