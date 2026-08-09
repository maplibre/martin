# Rendering cassette

What the styles in `tests/fixtures/styles` fetch while martin renders them, recorded so that
`integration-tests/tests/rendering.rs` never depends on those hosts being up or fast.

One directory per upstream host, mirroring the paths it serves:

```text
render_cassette/demotiles.maplibre.org/tiles/0/0/0.pbf
render_cassette/demotiles.maplibre.org/font/Open%20Sans%20Semibold/0-255.pbf
render_cassette/tiles.openfreemap.org/planet/_index
```

A path that is also a directory - `/planet` answers a TileJSON and holds the tiles it names - is
stored as the `_index` file inside it.

The tests serve this tree over HTTP under `/{host}/{path}` and hand martin copies of the styles
with their URLs pointed there.

A request that no recording covers is fetched from the upstream and written here, so a test that
renders something new records what it needs on its first local run:

```bash
just test-rendering
git add tests/fixtures/render_cassette
```

Delete a file and re-run to refresh it after an upstream rotates its assets. On CI the same request
is answered with a 404 and fails the test, so a cassette that is behind the tests shows up there
instead of turning into a download.
