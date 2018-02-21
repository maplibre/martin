use futures::future::{result, FutureResult};
use actix::{Addr, Syn};
use actix_web::{middleware, Application, Error, HttpRequest, HttpResponse, Method, Result};
use actix_web::error::ErrorNotFound;
use actix_web::httpcodes::HTTPOk;

use super::db::DbExecutor;
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

fn tile(req: HttpRequest<State>) -> FutureResult<HttpResponse, Error> {
    let z = req.match_info().get("z").unwrap();
    let x = req.match_info().get("x").unwrap();
    let y = req.match_info().get("y").unwrap();

    println!("{:?}", req);

    result(
        HttpResponse::Ok()
            .content_type("text/html")
            .body(format!("requested tile {} {} {}", z, x, y))
            .map_err(|e| e.into()),
    )
}

pub fn new(db_sync_arbiter: Addr<Syn, DbExecutor>, sources: Sources) -> Application<State> {
    let state = State {
        db: db_sync_arbiter,
        sources: sources,
    };

    Application::with_state(state)
        .middleware(middleware::Logger::default())
        .resource("/index.json", |r| r.method(Method::GET).f(index))
        .resource("/{source}.json", |r| r.method(Method::GET).f(source))
        .resource("/{source}/{z}/{x}/{y}.pbf", |r| {
            r.method(Method::GET).a(tile)
        })
}
