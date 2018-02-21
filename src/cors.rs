use iron::AfterMiddleware;
use iron::prelude::{Request, Response, IronResult};
use iron::headers::AccessControlAllowOrigin;

pub struct Middleware;

impl AfterMiddleware for Middleware {
    fn after(&self, _req: &mut Request, mut resp: Response) -> IronResult<Response> {
        resp.headers.set(AccessControlAllowOrigin::Any);
        Ok(resp)
    }
}
