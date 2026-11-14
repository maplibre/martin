# Martin-Core Examples

This directory contains examples demonstrating how to use `martin-core` to build custom tile servers.

| Example                                  | Demonstrates                                                                                           |
|------------------------------------------|--------------------------------------------------------------------------------------------------------|
| `cargo run --example pmtiles-tileserver` | A minimal tile server that serves raster tiles from a local PMTiles file with a custom MapLibre style. |
| `cargo run --release --features rendering --example render-bench -- <case> <style.json> [workers]` | Times the render pools on tiles, static images, overlays and style reloads; set `MARTIN_MLN_BACKEND` to time another graphics backend. |
