# Martin-Core Examples

This directory contains examples demonstrating how to use `martin-core` to build custom tile servers.

| Example                                                                                            | Demonstrates                                                                                           |
|----------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------|
| `cargo run --example pmtiles-tileserver`                                                           | A minimal tile server that serves raster tiles from a local PMTiles file with a custom MapLibre style. |
| `cargo run --release --features rendering --example render-bench -- <case> <style.json> [workers]` | Times Martin's render pools on one case and prints the result as one JSON line. Linux only.            |
| `cargo run --release --features rendering --example render-tileserver -- [--style <style.json>]`   | Serves tiles rendered from a MapLibre style on http://127.0.0.1:3000 with a Leaflet page. Linux only.  |

## render-bench

`render-bench` times one case per run, with `workers` render threads per pool (default 1).
Set `MARTIN_MLN_BACKEND` to `gl`, `vulkan` or `webgpu` to time that graphics backend.
Set `BENCH_SAVE=<dir>` to also write rendered images there as PNG: every image of `sweep`, `lowzoom` and `static`, and one image of `cold-tile` and `overlay`. The other cases write none.

| Case                             | Times                                                                 |
|----------------------------------|-----------------------------------------------------------------------|
| `startup`                        | spawning the render pools                                             |
| `cold-tile`                      | the first tile of a fresh pool, including style and resource loading  |
| `warm-tile[:<size>@<ratio>]`     | the same tile again and again (default `512@1`)                       |
| `sweep[:z<zoom>-<size>@<ratio>]` | distinct tiles one after another (default `z14-512@1`)                |
| `lowzoom`                        | every tile of zooms 0 to 3                                            |
| `static`                         | static images with bearing and pitch, at pixel ratio 2                |
| `overlay[:<features>]`           | a static image with an overlay of `features` features (default 60)    |
| `style-switch:<other.json>`      | two styles in turn on 1 worker, so every request reloads its style    |
| `throughput[:z<zoom>]`           | every tile of a block at once, in requests per second (default `z14`) |

```bash
MARTIN_MLN_BACKEND=vulkan cargo run --release --features rendering --example render-bench -- throughput:z14 style.json 8
```
