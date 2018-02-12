extern crate iron;
extern crate iron_test;
extern crate martin_lib;

use std::env;
use iron::Headers;
use iron_test::{request, response};

#[test]
fn test_index() {
    let conn_string: String = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let chain = martin_lib::chain(conn_string, 10, 0);

    let headers = Headers::new();
    let response = request::get("http://localhost:3000/index.json", headers, &chain).unwrap();

    let result_body = response::extract_body_to_bytes(response);
    assert!(result_body.len() > 0);
}
