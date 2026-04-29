//! Emit one of the generated schema artefacts on stdout — JSON Schema for the
//! config file, the HTTP `OpenAPI` 3.1 doc, or an annotated YAML config doc
//! built from the JSON Schema.
//!
//! Usage:
//!   `cargo run --quiet --no-default-features --features=unstable-schemas
//!     --bin gen-schemas -- --target config     | jq`
//!   `cargo run --quiet ... --bin gen-schemas -- --target openapi    | jq`
//!   `cargo run --quiet ... --bin gen-schemas -- --target config-doc`
//!
//! This binary only exists when the `unstable-schemas` feature is enabled —
//! see `martin/Cargo.toml` and `martin/src/schemas.rs`.

use std::io::Write as _;

use clap::{Parser, ValueEnum};
use martin::schemas::{config_doc_yaml, config_json_schema, openapi_spec};

/// Which document to emit on stdout.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
enum Target {
    /// JSON Schema for the on-disk Martin config file (`config.yaml`).
    Config,
    /// `OpenAPI` 3.1 document for Martin's HTTP API.
    Openapi,
    /// Annotated YAML config doc built from the JSON Schema (used to
    /// regenerate `docs/content/files/config.yaml`).
    ConfigDoc,
}

/// Emit a generated schema document to stdout.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// Document to emit.
    #[arg(long, value_enum)]
    target: Target,
}

fn main() {
    let args = Args::parse();
    let mut stdout = std::io::stdout().lock();

    match args.target {
        Target::Config => {
            let value = config_json_schema();
            serde_json::to_writer_pretty(&mut stdout, &value).expect("stdout");
            let _ = stdout.write_all(b"\n");
        }
        Target::Openapi => {
            let value = openapi_spec();
            serde_json::to_writer_pretty(&mut stdout, &value).expect("stdout");
            let _ = stdout.write_all(b"\n");
        }
        Target::ConfigDoc => {
            let _ = stdout.write_all(config_doc_yaml().as_bytes());
        }
    }
}
