//! Emit the generated JSON Schema (config) and `OpenAPI` spec (HTTP API) to
//! stdout, one document per `--target` invocation.
//!
//! Usage:
//!   `cargo run --quiet --no-default-features
//!     --features=unstable-schemas,mbtiles,pmtiles,postgres,sprites,styles,fonts,metrics
//!     --bin gen-schemas -- --target config | jq`
//!   `cargo run --quiet ... --bin gen-schemas -- --target openapi | jq`
//!
//! This binary only exists when the `unstable-schemas` feature is enabled —
//! see `martin/Cargo.toml` and `martin/src/schemas.rs`.

use std::io::Write as _;

use martin::schemas::{config_json_schema, openapi_spec};

#[derive(Debug)]
enum Target {
    Config,
    Openapi,
}

fn parse_args() -> Result<Target, String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--target" {
            return match args.next().as_deref() {
                Some("config") => Ok(Target::Config),
                Some("openapi") => Ok(Target::Openapi),
                Some(other) => Err(format!("unknown --target {other:?}")),
                None => Err("--target requires a value".to_string()),
            };
        }
    }
    Err("missing --target {config|openapi}".to_string())
}

fn main() {
    let target = match parse_args() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };

    let value = match target {
        Target::Config => config_json_schema(),
        Target::Openapi => openapi_spec(),
    };

    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, &value).expect("stdout");
    let _ = stdout.write_all(b"\n");
}
