extern crate iron;
extern crate router;

use iron::prelude::*;
use iron::status;
use router::Router;

fn get_json(req: &mut Request) -> IronResult<Response> {
    let params = req.extensions.get::<Router>().unwrap();

    println!("{} {} {}", req.method, req.version, req.url);
    println!("params is: {:?}", params);

    Ok(Response::with((status::Ok, "ok")))
}

fn main() {
    let mut router = Router::new();
    router.get("/:schema/:table.json", get_json, "get_json");

    let port = 3000;
    let bind_addr = format!("localhost:{}", port);
    println!("Server started on {}.", bind_addr);
    Iron::new(router).http(bind_addr.as_str()).unwrap();
}