//! Writes `cog_tiles.pyi` from the bindings, run by `just sync-cog-py-stubs`.

use std::fs;

fn main() -> pyo3_stub_gen::Result<()> {
    _cog_tiles::stub_info()?.generate()?;
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/cog_tiles.pyi");
    // https://github.com/Jij-Inc/pyo3-stub-gen/issues/384: `create_exception!` bases are typed as builtins.
    let stub = fs::read_to_string(path)?.replace("builtins.CogTilesError", "CogTilesError");
    // The pre-commit hooks strip trailing whitespace, so write what they would leave.
    let lines: Vec<_> = stub.lines().map(str::trim_end).collect();
    fs::write(path, format!("{}\n", lines.join("\n").trim_end()))?;
    Ok(())
}
