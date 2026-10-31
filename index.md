![Martin](<https://raw.githubusercontent.com/maplibre/martin/main/logo.png>)

[![docs.rs docs](<https://docs.rs/martin/badge.svg>)](<https://docs.rs/martin>) [![join our community](<https://img.shields.io/badge/Slack-%23maplibre--martin-blueviolet?logo=slack>)](<https://slack.openstreetmap.us/>) [![GitHub](<https://img.shields.io/badge/github-maplibre/martin-8da0cb?logo=github>)](<https://github.com/maplibre/martin>) [![crates.io version](<https://img.shields.io/crates/v/martin.svg>)](<https://crates.io/crates/martin>) [![CI build](<https://github.com/maplibre/martin/actions/workflows/ci.yml/badge.svg>)](<https://github.com/maplibre/martin/actions>) [![OpenSSF Best Practices](<https://www.bestpractices.dev/projects/11613/badge>)](<https://www.bestpractices.dev/projects/11613>)

Martin is a tile server optimized for speed and heavy traffic, written in [Rust](<https://github.com/rust-lang/rust>).

- **Quick Start**

  ---

  [Install](<https://maplibre.org/martin/installation/index.md>) a binary, a Docker image, or a package, and serve your first tiles within a few minutes.
- **Tile sources**

  ---

  [PostGIS](<https://maplibre.org/martin/pg-connections/index.md>) tables and functions with automatic discovery, [PMTiles](<https://maplibre.org/martin/sources-pmtiles/index.md>), [MBTiles](<https://maplibre.org/martin/sources-mbtiles/index.md>), [COG](<https://maplibre.org/martin/sources-cog-files/index.md>), [GeoJSON](<https://maplibre.org/martin/sources-geojson/index.md>), and [GeoParquet](<https://maplibre.org/martin/sources-duckdb/index.md>), [combined](<https://maplibre.org/martin/sources-composite/index.md>) into one source where you need it.
- **Styles, sprites &amp; fonts**

  ---

  Serve [styles](<https://maplibre.org/martin/sources-styles/index.md>) and generate [sprites](<https://maplibre.org/martin/sources-sprites/index.md>) and [font glyphs](<https://maplibre.org/martin/sources-fonts/index.md>) on the fly, so a map needs nothing but Martin.
- **Bulk generation &amp; tooling**

  ---

  Generate tiles into an archive with [martin cp](<https://maplibre.org/martin/martin-cp/index.md>), then examine, copy, validate, and diff it with [mbtiles](<https://maplibre.org/martin/mbtiles/index.md>).
- **Run it anywhere**

  ---

  A single binary on a server, in [Docker](<https://maplibre.org/martin/run-with-docker/index.md>), on [AWS Lambda](<https://maplibre.org/martin/run-with-lambda/index.md>), or behind [NGINX](<https://maplibre.org/martin/run-with-nginx/index.md>) and [Apache](<https://maplibre.org/martin/run-with-apache/index.md>).
- ![The Martin terminal dashboard, showing per-source request counts and timings, a world map of recent tile requests, a request-rate chart, and a log pane](<https://maplibre.org/martin/images/tui-dashboard.webp>)

  **Terminal &amp; web UI**

  ---

  Useful for monitoring, debugging and development.

[Explore on our demo site](<https://martin.maplibre.org/>) [Use it with your map](<https://maplibre.org/martin/using-guides/index.md>) [API endpoints](<https://maplibre.org/martin/using/index.md>)
