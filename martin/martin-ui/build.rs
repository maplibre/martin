#![expect(
    clippy::panic,
    reason = "panics in build.rs generate better error messages, are easier to debug and are fatal anyway"
)]

use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    let dist = Path::new("dist");
    if dist.join("index.html").is_file() {
        println!("cargo::rerun-if-changed=dist");
        static_files::resource_dir(dist)
            .build()
            .expect("failed to embed the martin-ui dist directory");
        return;
    }

    // `package.json` is left out of the published crate, so its absence means a crate packaged without a bundle.
    assert!(
        Path::new("package.json").is_file(),
        "martin-ui was packaged without dist/: run `just ui::build` before `cargo package` or `cargo publish`"
    );
    // A missing path makes cargo rerun on every build; the crate directory exists and changes once `dist/` appears.
    println!("cargo::rerun-if-changed=.");
    println!(
        "cargo::warning=martin-ui/dist is missing, embedding a placeholder page. Run `just ui::build` for the real web UI."
    );
    let stub = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo")).join("stub-webui");
    fs::create_dir_all(&stub).unwrap_or_else(|e| panic!("failed to create {}: {e}", stub.display()));
    fs::write(
        stub.join("index.html"),
        "This Martin build has no web UI bundle. Run `just ui::build` and rebuild to embed it.\n",
    )
    .expect("failed to write the placeholder web UI page");
    static_files::resource_dir(&stub)
        .build()
        .expect("failed to embed the placeholder web UI page");
}
