## Using with OpenLayers

[OpenLayers](https://github.com/openlayers/openlayers) is a high-performance, feature-packed library for creating interactive maps on the web. It can display map tiles, vector data and markers loaded from any source on any web page. OpenLayers has been developed to further the use of geographic information of all kinds. It is completely free, Open Source JavaScript, released under the BSD 2-Clause License.

You can integrate tile services from `martin` and `OpenLayers` with its [VectorTileLayer](https://openlayers.org/en/latest/apidoc/module-ol_layer_VectorTile-VectorTileLayer.html).

```js
const layer = new VectorTileLayer({
    source: new VectorTileSource({
        format: new MVT(),
        url:
            'http://0.0.0.0:3000/MixPoints/{z}/{x}/{y}',
        maxZoom: 14,
    }),
});
map.addLayer(layer);
```
