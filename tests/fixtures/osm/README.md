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
- `split-string-table.osm.pbf`: two zlib blocks whose string tables go on after their first group, as the PBF format
  allows. Block 1 holds one group with node 1 (`amenity=cafe`), its two strings after the group. Block 2 holds the
  strings `amenity` and `cafe`, two groups with the untagged nodes 3 and 4, then the string `x`. A read filtering on
  `amenity == 'cafe'` skips block 1 by the strings before its group and counts block 2.
- `shortbread.spec.yml`: Planetiler's examples of its Shortbread profile, unchanged
  (`planetiler-custommap/src/main/resources/samples/shortbread.spec.yml`, commit `ee9f808cf726b7e57a0017320a107e95087a85a9`),
  under the [Apache License 2.0](https://github.com/onthegomap/planetiler/blob/main/LICENSE).
- `shortbread-examples.osm.pbf`: one OSM object per example of `shortbread.spec.yml`, written by
  `uv run --with osmium --with pyyaml python tests/fixtures/osm/shortbread_examples.py`.
- `shortbread-positions.osm.pbf`: three ways whose point in a Shortbread point layer depends on their shape,
  written by `uv run --with osmium python tests/fixtures/osm/shortbread_positions.py`.
- `shortbread-monaco.spec.yaml`: the [`martin test`](../../../docs/content/martin-test.md) spec of
  `docs/snippets/shortbread.yaml` on `monaco.osm.pbf`, written from the workspace root by

  ```bash
  sed 's#berlin-latest.osm.pbf#tests/fixtures/osm/monaco.osm.pbf#' docs/snippets/shortbread.yaml > /tmp/shortbread.yaml
  martin test generate --config /tmp/shortbread.yaml --output tests/fixtures/osm/shortbread-monaco.spec.yaml
  ```
