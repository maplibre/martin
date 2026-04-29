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

use clap::{Parser, ValueEnum};
use martin::schemas::{config_json_schema, openapi_spec};

/// Which schema document to emit on stdout.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lowercase")]
enum Target {
    /// JSON Schema for the on-disk Martin config file (`config.yaml`).
    Config,
    /// `OpenAPI` 3.1 document for Martin's HTTP API.
    Openapi,
}

/// Emit a generated schema document to stdout as pretty-printed JSON.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// Document to emit.
    #[arg(long, value_enum)]
    target: Target,
}

fn main() {
    let args = Args::parse();

    let value = match args.target {
        Target::Config => config_json_schema(),
        Target::Openapi => openapi_spec(),
    };

    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, &value).expect("stdout");
    let _ = stdout.write_all(b"\n");
}
