# Martin-Core Examples

This directory contains examples demonstrating how to use `martin-core` to build custom tile servers.

| Example                                                                                            | Demonstrates                                                                                           |
|----------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------|
| `cargo run --example pmtiles-tileserver`                                                           | A minimal tile server that serves raster tiles from a local PMTiles file with a custom MapLibre style. |
| `cargo run --release --features rendering --example render-tileserver -- [--style <style.json>]`   | Serves tiles rendered from a MapLibre style on http://127.0.0.1:3000 with a Leaflet page. Linux only.  |
