!!! warning "Not released or fully implemented"
    `martin generate` is not in any release yet, and parts of what these pages describe are not
    implemented yet.
    It is only built with the `unstable-generate` feature:

    ```bash
    cargo build --package martin --bin martin --features unstable-generate
    ```

    Not implemented yet:

    - [ ] Rendering points, lines and polygons, with fill ranges for covered tiles
    - [ ] Tile grouping, MLT and MVT encoding, compression and deduplication
    - [ ] Planning tables into layers and running a whole generation end to end
    - [ ] The PostgreSQL source with partitioned whole-table scans
    - [ ] The `martin generate` command and PMTiles output
    - [ ] Layers with CEL expressions, `rules` and `sort_by`
    - [ ] OSM PBF, shapefile, GeoPackage and CSV files as sources
    - [ ] `geometry` other than `point`, `line` and `polygon`
    - [ ] `tile` operations (`merge_lines`, `merge_polygons`, `merge_multi`, `label_grid`, `dedup_by`, `limit`)
    - [ ] `prefetch`
