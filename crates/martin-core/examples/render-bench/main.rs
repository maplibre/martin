//! Times Martin's render pools on the operations Martin serves; one case per process.
//!
//! `render-bench <case> <style.json> [workers]`, printing one JSON line.
//! Set `BENCH_SAVE=<dir>` to also write rendered images there as PNG; `examples/README.md` lists which cases save.

#[cfg(target_os = "linux")]
mod bench;

/// Runs the benchmark case the command line names.
#[cfg(target_os = "linux")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    bench::run().await
}

/// Fails, as Martin renders only on Linux.
#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("render-bench needs Linux, the only platform Martin renders on")
}
