//! The middleware that feeds requests to the dashboard.

use std::time::Instant;

use actix_web::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::middleware::Next;
use actix_web::{Error, HttpRequest};

use super::data::TileRequest;

/// Records every request on the installed dashboard, with the tile it asked for when it asked for one.
pub async fn observe(
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    let started = Instant::now();
    let res = next.call(req).await?;
    if let Some(dashboard) = super::installed() {
        dashboard.record(tile_request(res.request()), res.status(), started.elapsed());
    }
    Ok(res)
}

/// The tile a matched request asked for.
fn tile_request(req: &HttpRequest) -> Option<TileRequest> {
    let info = req.match_info();
    let source = info.get("source_ids").or_else(|| info.get("ids"))?;
    Some(TileRequest {
        source: source.to_owned(),
        z: info.get("z")?.parse().ok()?,
        x: info.get("x")?.parse().ok()?,
        y: info.get("y")?.parse().ok()?,
    })
}
