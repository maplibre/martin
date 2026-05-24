# Rendering test HTTP cassette

The rendering unit tests in `martin-core` want to hit `demotiles.maplibre.org` and `tiles.openfreemap.org` for more realistic tests,
this is flaky in CI

[mitmproxy](https://docs.mitmproxy.org) runs as a plain-HTTP reverse-proxy in front of those two upstreams.
The test code rewrites style JSON URLs to point at it, and CI
replays the `flows` cassette in this directory.

> [!TIP]
> The cassete can be refreshed by running
>
> ```bash
> just seed-render-fixtures
> git add tests/fixtures/rendering_cache/flows
> ```
>
> Refreshing may be required if style fixtures gain/lose URLs or upstream assets rotate.
