//! Writes `cog_tiles.pyi` from the bindings, run by `just sync-cog-py-stubs`.

use std::fs;

fn main() -> pyo3_stub_gen::Result<()> {
    _cog_tiles::stub_info()?.generate()?;
    // https://github.com/Jij-Inc/pyo3-stub-gen/issues/384: `create_exception!` bases are typed as builtins.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/cog_tiles.pyi");
    let stub = fs::read_to_string(path)?;
    fs::write(path, stub.replace("builtins.CogTilesError", "CogTilesError"))?;
    Ok(())
}
