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
This guide serves all of California 2020 as one Martin source with a [tile index](sources-cog-files.md#imagery-listed-by-a-tile-index), and shows the Bay Area on a MapLibre map with [OpenFreeMap](https://openfreemap.org) roads on top.

## Prerequisites

- [Martin](installation.md), built with the `cog` feature (on by default)
- The [AWS CLI](https://aws.amazon.com/cli/) and the [GDAL](https://gdal.org) command line tools, for `ogr2ogr`
- AWS credentials allowed to read `s3://naip-visualization`.
  The bucket is **requester pays**: every request is billed to your AWS account, so a request without `--request-payer requester` (or Martin's `aws_request_payer: true`) is denied.

Check your access with a 4 KB range read:

```bash
aws s3api get-object --bucket naip-visualization \
  --key ca/2020/60cm/rgb/37122/m_3712213_nw_10_060_20200524.tif \
  --request-payer requester --range bytes=0-4095 header.tif
```

## Build the tile index

Each state and year comes with an index of its own: a shapefile with the footprint of every quarter-quadrangle, e.g. `ca/2020/60cm/index/NAIP_20_CA.shp`.
Its `FileName` is not quite the file's key, though: it lacks the folder, and ends in an extra date, the version date.
For a few files, its image date is also a day off from the key's.
So the index is joined to a listing of the files by their names up to the date, which gives every footprint its real key:

```bash
export AWS_REGION=us-west-2 AWS_REQUEST_PAYER=requester

# Every file the bucket has, with its name up to the date, e.g. `m_3712213_nw_10_060`.
aws s3api list-objects-v2 --bucket naip-visualization --request-payer requester \
  --prefix ca/2020/60cm/rgb/ --query 'Contents[].Key' --output text | tr '\t' '\n' \
  | awk -F/ 'BEGIN { print "key,qq" } /\.tif$/ { print $0 "," substr($NF, 1, 19) }' > files.csv

# The bucket's footprints and the listing, side by side.
ogr2ogr -f GPKG -nln footprints work.gpkg /vsis3/naip-visualization/ca/2020/60cm/index/NAIP_20_CA.shp
ogr2ogr -update -nln files work.gpkg files.csv

# One row per file: its footprint, and where to read it.
ogr2ogr -f GPKG -nln naip naip.gpkg work.gpkg -dialect SQLite -sql \
  "SELECT '/vsis3/naip-visualization/' || f.key AS location, p.geom
   FROM footprints p JOIN files f ON f.qq = substr(p.FileName, 1, 19)"
```

This takes about 20 seconds for the 11,076 files of California 2020, and reads no file but the index.
For another state or year, list its `index/` folder for the shapefile's name.

!!! tip
    Imagery without an index of its own can be indexed with `gdaltindex -f GPKG -t_srs EPSG:4326`, which reads the header of every file.

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
curl -o tile.png http://localhost:3000/naip/18/41886/101282
```

The first tile of an area takes a few seconds while Martin opens the files; later tiles reuse them.
Zooms outside `minzoom` to `maxzoom` are answered with `404` without reading from S3.

## Show it on a map

This page shows the Golden Gate Bridge at zoom 18 with the roads and labels of OpenFreeMap on top, motorways to secondary roads in pink.
`hash: true` keeps the map position in the URL, e.g. `#14/37.93/-122.69`.

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
  z <code id="z"></code> naip tile <code id="tile"></code>
  <label><input type="checkbox" id="ofm" checked> OpenFreeMap roads + labels</label>
  <button data-go="-122.4783,37.8199">Golden Gate</button>
  <button data-go="-122.3937,37.7955">Ferry Building</button>
  <button data-go="-122.4230,37.8267">Alcatraz</button>
  <button data-go="-122.2585,37.8024">Lake Merritt</button>
</div>
<script type="module">
import { Map, NavigationControl } from "https://unpkg.com/maplibre-gl@6.14.0/dist/maplibre-gl.mjs";

const CENTER = [-122.4783, 37.8199];
const BIG_ROADS = ["motorway", "trunk", "primary", "secondary"];

// MapLibre line widths are pixels of a 512 px tile world; at this latitude a meter is this many px at zoom z.
const pxPerMeter = z => (512 * 2 ** z) / (40075016.686 * Math.cos(CENTER[1] * Math.PI / 180));
const meters = m => ["interpolate", ["exponential", 2], ["zoom"], 10, m * pxPerMeter(10), 22, m * pxPerMeter(22)];

const BOUNDS = [[-122.65, 37.60], [-122.10, 38.00]];
const bigRoad = ["in", ["get", "class"], ["literal", BIG_ROADS]];
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
    },
    layers: [
      { id: "bg", type: "background", paint: { "background-color": "#111" } },
      { id: "naip", type: "raster", source: "naip" },
      {
        id: "ofm-roads", type: "line", source: "ofm", "source-layer": "transportation",
        filter: ["!", bigRoad],
        paint: { "line-color": "#ffffff", "line-opacity": 0.45, "line-width": ["interpolate", ["linear"], ["zoom"], 14, 0.5, 20, 2] },
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
map.addControl(new NavigationControl(), "top-right");

// The NAIP tile MapLibre requests at the center: its tiles are 256 px, so one zoom above the map's 512 px zoom.
function update() {
  const z = Math.min(Math.floor(map.getZoom()) + 1, map.getSource("naip").maxzoom), c = map.getCenter(), n = 2 ** z;
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
document.querySelectorAll("[data-go]").forEach(b => b.onclick = () =>
  map.flyTo({ center: b.dataset.go.split(",").map(Number), zoom: 18 }));
</script>
</body>
</html>
```
