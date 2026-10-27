---
description: NAIP aerial imagery from a requester-pays S3 bucket via a tile index
icon: material/airplane
tags:
  - cog
  - tile-sources
  - recipes
---

# Serving NAIP aerial imagery from S3 with a tile index

The [National Agriculture Imagery Program](https://registry.opendata.aws/naip/) (NAIP) publishes aerial imagery of the United States as COGs in the `naip-visualization` S3 bucket: one file per USGS quarter-quadrangle, each in its own UTM zone, overlapping its neighbors.
This guide serves a few of those files as one Martin source with a [tile index](sources-cog-files.md#imagery-listed-by-a-tile-index), and shows them on a MapLibre map with [OpenFreeMap](https://openfreemap.org) roads on top.

## Prerequisites

- [Martin](installation.md), built with the `cog` feature (on by default)
- The [GDAL](https://gdal.org) command line tools, for `gdaltindex`
- AWS credentials allowed to read `s3://naip-visualization`.
  The bucket is **requester pays**: every request is billed to your AWS account, so a request without `--request-payer requester` (or Martin's `aws_request_payer: true`) is denied.

Check your access with a 4 KB range read:

```bash
aws s3api get-object --bucket naip-visualization \
  --key ca/2020/60cm/rgb/37122/m_3712203_ne_10_060_20200524.tif \
  --request-payer requester --range bytes=0-4095 header.tif
```

## Build the tile index

`gdaltindex` reads the header of each file and writes its footprint and location into a GeoPackage.
These are the four quarter-quads of quad `3712203` in Marin County, California:

```bash
export AWS_REGION=us-west-2 AWS_REQUEST_PAYER=requester
gdaltindex -f GPKG -t_srs EPSG:4326 -src_srs_name src_srs naip.gpkg \
  /vsis3/naip-visualization/ca/2020/60cm/rgb/37122/m_3712203_ne_10_060_20200524.tif \
  /vsis3/naip-visualization/ca/2020/60cm/rgb/37122/m_3712203_nw_10_060_20200602.tif \
  /vsis3/naip-visualization/ca/2020/60cm/rgb/37122/m_3712203_se_10_060_20200524.tif \
  /vsis3/naip-visualization/ca/2020/60cm/rgb/37122/m_3712203_sw_10_060_20200602.tif
```

`-t_srs EPSG:4326` is required: Martin reads footprints in longitude and latitude only.
For a whole state and year, list the keys first and pass them with `--optfile`:

```bash
aws s3 ls --recursive --request-payer requester s3://naip-visualization/ca/2020/60cm/rgb/ \
  | awk '/\.tif$/ {print "/vsis3/naip-visualization/" $4}' > files.txt
gdaltindex -f GPKG -t_srs EPSG:4326 -src_srs_name src_srs naip.gpkg --optfile files.txt
```

## Configure Martin

```yaml title="config.yaml"
cog:
  # An AWS SDK profile with read access to the bucket; any other way of passing credentials works too.
  profile: naip
  aws_region: us-west-2
  # The bucket is requester pays.
  aws_request_payer: true
  tile_index:
    naip:
      path: naip.gpkg
      minzoom: 14
      maxzoom: 19
```

```bash
martin --config config.yaml
curl -o tile.png http://localhost:3000/naip/18/41733/101174
```

The first tile of an area takes a few seconds while Martin opens the files; later tiles reuse them.
Zooms outside `minzoom` to `maxzoom` are answered with `404` without reading from S3.

## Show it on a map

This page shows the imagery at zoom 18 with the roads and labels of OpenFreeMap on top.
Every motorway, trunk, primary and secondary road gets a translucent pink band reaching 250 m to each side.

- The band is drawn with `line-layer-opacity` (MapLibre GL JS 6), so overlapping segments do not add up to a darker pink.
- OpenFreeMap tiles only hold a few meters of geometry beyond their edges, so a band drawn from them would stop at each tile edge.
  The page instead reads the big roads from the zoom-12 tiles around the area into a GeoJSON source, whose tiles each hold far more than 250 m of their neighbors.
- `hash: true` keeps the map position in the URL, e.g. `#14/37.93/-122.69`.

Save it as `index.html` and serve it over HTTP, e.g. with `python3 -m http.server 8000`, then open <http://localhost:8000>.

```html title="index.html"
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>NAIP z18 demo</title>
<link rel="stylesheet" href="https://unpkg.com/maplibre-gl@6.14.0/dist/maplibre-gl.css">
<style>
  :root { --bg: #000; --fg: #fff; --accent: #ff4fa3; }
  html, body { margin: 0; height: 100%; background: var(--bg); color: var(--fg); font: 13px/1.4 system-ui, sans-serif; }
  #map { position: absolute; inset: 0; }
  #panel { position: absolute; top: 12px; left: 12px; background: var(--bg); border: 2px solid var(--accent);
           border-radius: 2px; padding: 8px 10px; z-index: 1; min-width: 230px; }
  #panel b { color: var(--accent); }
  #panel label { display: block; margin-top: 6px; cursor: pointer; }
  #panel button { margin: 6px 6px 0 0; background: var(--accent); color: #000; border: 0; border-radius: 2px;
                  padding: 4px 8px; font: inherit; cursor: pointer; }
  code { color: var(--accent); }
</style>
</head>
<body>
<div id="map"></div>
<div id="panel">
  <b>NAIP CA 2020 / martin tile_index</b><br>
  source <code>localhost:3000/naip</code><br>
  z <code id="z"></code> tile <code id="tile"></code>
  <label><input type="checkbox" id="ofm" checked> OpenFreeMap roads + labels</label>
  <label><input type="checkbox" id="buffer" checked> 250 m road buffer</label>
  <label><input type="checkbox" id="grid"> tile grid</label>
  <button data-go="-122.6875,37.9375">4-way seam</button>
  <button data-go="-122.6851,37.97">nw/ne edge</button>
  <button data-go="-122.72,37.96">inside nw</button>
</div>
<script type="module">
import { Map, NavigationControl } from "https://unpkg.com/maplibre-gl@6.14.0/dist/maplibre-gl.mjs";
import { VectorTile } from "https://esm.sh/@mapbox/vector-tile@2.0.3";
import Pbf from "https://esm.sh/pbf@4.0.1";

const CENTER = [-122.6875, 37.9375];
const BUFFER_M = 250;
const BIG_ROADS = ["motorway", "trunk", "primary", "secondary"];

// MapLibre line widths are pixels of a 512 px tile world; at this latitude a meter is this many px at zoom z.
const pxPerMeter = z => (512 * 2 ** z) / (40075016.686 * Math.cos(CENTER[1] * Math.PI / 180));
const meters = m => ["interpolate", ["exponential", 2], ["zoom"], 10, m * pxPerMeter(10), 22, m * pxPerMeter(22)];

const BOUNDS = [[-122.80, 37.85], [-122.58, 38.03]];
const BAND_ZOOM = 12;
const bigRoad = ["in", ["get", "class"], ["literal", BIG_ROADS]];

// The big roads of every OpenFreeMap tile at BAND_ZOOM over BOUNDS, so the band sees roads beyond each tile's edge.
async function bigRoadsOver(tileJsonUrl) {
  const { tiles: [template] } = await (await fetch(tileJsonUrl)).json();
  const n = 2 ** BAND_ZOOM;
  const tx = lng => Math.floor((lng + 180) / 360 * n);
  const ty = lat => Math.floor((1 - Math.asinh(Math.tan(lat * Math.PI / 180)) / Math.PI) / 2 * n);
  const coords = [];
  for (let x = tx(BOUNDS[0][0]); x <= tx(BOUNDS[1][0]); x++)
    for (let y = ty(BOUNDS[1][1]); y <= ty(BOUNDS[0][1]); y++) coords.push([x, y]);
  const features = await Promise.all(coords.map(async ([x, y]) => {
    const url = template.replace("{z}", BAND_ZOOM).replace("{x}", x).replace("{y}", y);
    const layer = new VectorTile(new Pbf(await (await fetch(url)).arrayBuffer())).layers.transportation;
    const out = [];
    for (let i = 0; layer && i < layer.length; i++) {
      const f = layer.feature(i);
      if (BIG_ROADS.includes(f.properties.class) && f.type === 2) out.push(f.toGeoJSON(x, y, BAND_ZOOM));
    }
    return out;
  }));
  return { type: "FeatureCollection", features: features.flat() };
}
const ofmLayers = ["ofm-roads", "ofm-big-roads", "ofm-road-names", "ofm-places"];

const map = new Map({
  container: "map",
  center: CENTER,
  zoom: 18,
  hash: true,
  minZoom: 14,
  maxZoom: 19,
  maxBounds: BOUNDS,
  style: {
    version: 8,
    glyphs: "https://tiles.openfreemap.org/fonts/{fontstack}/{range}.pbf",
    sources: {
      naip: { type: "raster", tiles: ["http://localhost:3000/naip/{z}/{x}/{y}"], tileSize: 256,
              minzoom: 14, maxzoom: 19, attribution: "NAIP 2020 via martin" },
      ofm: { type: "vector", url: "https://tiles.openfreemap.org/planet",
             attribution: "<a href=\"https://openfreemap.org\">OpenFreeMap</a> &copy; OpenMapTiles" },
      // Tiled only up to BAND_ZOOM and overzoomed after, so each tile's buffer spans far more than 250 m.
      bigRoads: { type: "geojson", data: { type: "FeatureCollection", features: [] },
                  maxzoom: BAND_ZOOM, buffer: 512, tolerance: 0 },
    },
    layers: [
      { id: "bg", type: "background", paint: { "background-color": "#111" } },
      { id: "naip", type: "raster", source: "naip" },
      {
        id: "road-buffer", type: "line", source: "bigRoads",
        layout: { "line-cap": "round", "line-join": "round" },
        paint: { "line-color": "#ff4fa3", "line-width": meters(2 * BUFFER_M), "line-layer-opacity": 0.18 },
      },
      {
        id: "ofm-roads", type: "line", source: "ofm", "source-layer": "transportation",
        filter: ["!", bigRoad],
        paint: { "line-color": "#ffffff", "line-opacity": 0.45, "line-width": meters(4) },
      },
      {
        id: "ofm-big-roads", type: "line", source: "ofm", "source-layer": "transportation",
        filter: bigRoad,
        layout: { "line-cap": "round", "line-join": "round" },
        paint: { "line-color": "#ff4fa3", "line-opacity": 0.9, "line-width": ["interpolate", ["exponential", 2], ["zoom"], 14, 2, 22, 3 * pxPerMeter(22)] },
      },
      {
        id: "ofm-road-names", type: "symbol", source: "ofm", "source-layer": "transportation_name",
        layout: { "symbol-placement": "line", "text-field": ["coalesce", ["get", "name:en"], ["get", "name"]],
                  "text-font": ["Noto Sans Regular"], "text-size": 14 },
        paint: { "text-color": "#fff", "text-halo-color": "#000", "text-halo-width": 1.5 },
      },
      {
        id: "ofm-places", type: "symbol", source: "ofm", "source-layer": "place",
        layout: { "text-field": ["coalesce", ["get", "name:en"], ["get", "name"]],
                  "text-font": ["Noto Sans Regular"], "text-size": 15 },
        paint: { "text-color": "#ff4fa3", "text-halo-color": "#000", "text-halo-width": 1.5 },
      },
    ],
  },
});
bigRoadsOver("https://tiles.openfreemap.org/planet").then(data => map.getSource("bigRoads").setData(data));
map.addControl(new NavigationControl(), "top-right");

function update() {
  const z = Math.floor(map.getZoom()), c = map.getCenter(), n = 2 ** z;
  const x = Math.floor((c.lng + 180) / 360 * n);
  const r = c.lat * Math.PI / 180;
  const y = Math.floor((1 - Math.asinh(Math.tan(r)) / Math.PI) / 2 * n);
  document.getElementById("z").textContent = map.getZoom().toFixed(1);
  document.getElementById("tile").textContent = `${z}/${x}/${y}`;
}
map.on("move", update);
map.on("load", update);

const visible = on => (on ? "visible" : "none");
document.getElementById("ofm").onchange = e =>
  ofmLayers.forEach(id => map.setLayoutProperty(id, "visibility", visible(e.target.checked)));
document.getElementById("buffer").onchange = e =>
  map.setLayoutProperty("road-buffer", "visibility", visible(e.target.checked));
document.getElementById("grid").onchange = e => { map.showTileBoundaries = e.target.checked; };
document.querySelectorAll("[data-go]").forEach(b => b.onclick = () =>
  map.flyTo({ center: b.dataset.go.split(",").map(Number), zoom: 18 }));
</script>
</body>
</html>
```
