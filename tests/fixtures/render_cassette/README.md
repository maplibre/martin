# Rendering cassette

What the styles in `tests/fixtures/styles` fetch while martin renders them, recorded so that
`integration-tests/tests/rendering.rs` never depends on those hosts being up or fast.

One directory per upstream host, mirroring the paths it serves:

```text
render_cassette/demotiles.maplibre.org/tiles/0/0/0.pbf
render_cassette/demotiles.maplibre.org/font/Open%20Sans%20Semibold/0-255.pbf
```

The test serves this directory over HTTP and hands martin a copy of the style whose URLs point
there.

A request that no recording covers is fetched from the upstream and written here, so a test that
renders something new records what it needs on its first local run:

```bash
just test-rendering
git add tests/fixtures/render_cassette
```

Delete a file and re-run to refresh it after an upstream rotates its assets. On CI the same request
is answered with a 404 and fails the test, so a cassette that is behind the tests shows up there
instead of turning into a download.
