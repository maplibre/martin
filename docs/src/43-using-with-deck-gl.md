# Using with deck.gl

[deck.gl](https://deck.gl/) is a WebGL-powered framework for visual exploratory data analysis of large datasets.

You can add vector tiles using [MVTLayer](https://deck.gl/docs/api-reference/geo-layers/mvt-layer). MVTLayer `data` property defines the remote data for the MVT layer. It can be

- `String`: Either a URL template or a [TileJSON](https://github.com/mapbox/tilejson-spec) URL.
- `Array`: an array of URL templates. It allows to balance the requests across different tile endpoints. For example, if you define an array with 4 urls and 16 tiles need to be loaded, each endpoint is responsible to server 16/4 tiles.
- `JSON`: A valid [TileJSON object](https://github.com/mapbox/tilejson-spec/tree/master/2.2.0).

```js
const pointsLayer = new MVTLayer({
  data: 'http://localhost:3000/points',
  pointRadiusUnits: 'pixels',
  getRadius: 5,
  getFillColor: [230, 0, 0]
});

const deckgl = new DeckGL({
  container: 'map',
  mapStyle: 'https://basemaps.cartocdn.com/gl/dark-matter-gl-style/style.json',
  initialViewState: {
    latitude: 0,
    longitude: 0,
    zoom: 1
  },
  layers: [pointsLayer]
});
```
