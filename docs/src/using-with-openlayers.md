## Using with OpenLayers

[OpenLayers](https://github.com/openlayers/openlayers) is an open source library for creating interactive maps on the web. Similar to [MapLibre GL JS](https://maplibre.org/), it can also display image and vector map tiles served by Martin Tile Server.

You can integrate tile services from `martin` and `OpenLayers` with its [VectorTileLayer](https://openlayers.org/en/latest/apidoc/module-ol_layer_VectorTile-VectorTileLayer.html). Here is an example to add `MixPoints` vector tile source to an OpenLayers map.

```js
const layer = new VectorTileLayer({
    source: new VectorTileSource({
        format: new MVT(),
        url: 'http://0.0.0.0:3000/MixPoints/{z}/{x}/{y}',
        maxZoom: 14,
    }),
});
map.addLayer(layer);
```
