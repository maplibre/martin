# OpenStreetMap fixtures

- `monaco.osm.pbf`: Geofabrik's extract of Monaco, as Planetiler's tests have it
  (`planetiler-core/src/test/resources/monaco-latest.osm.pbf`, commit `21f3d5d03bcf1e4b11704e9be699aa8676c0ff22`).
  The data is © OpenStreetMap contributors, under the [ODbL](https://opendatacommons.org/licenses/odbl/).
- `unsorted.osm.pbf`: two nodes, node 2 before node 1, in one uncompressed block, with no `Sort.Type_then_ID` in the header.
  Written by `write_pbf` in `martin-tilegen/tests/osm_edge_cases.rs` from this OPL:

  ```text
  n2 x7.4200 y43.7300 Tname=second
  n1 x7.4100 y43.7300 Tname=first
  ```
