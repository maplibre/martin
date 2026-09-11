import type { Map as MapLibre, RequestTransformFunction, VectorTileSource } from 'maplibre-gl';

const MLT_MIME = 'application/vnd.maplibre-tile';

function servesMlt(map: MapLibre | undefined, url: string): boolean {
  if (!map?.isStyleLoaded()) return false;
  return Object.keys(map.getStyle().sources).some((id) => {
    const source = map.getSource(id) as VectorTileSource | undefined;
    return (
      source?.type === 'vector' &&
      source.encoding === 'mlt' &&
      source.tiles?.some((tile) => url.startsWith(tile.split('{')[0]))
    );
  });
}

/**
 * Workaround for maplibre-gl not sending an `Accept` header on tile requests, which Martin
 * requires to serve MLT. Delete once https://github.com/maplibre/maplibre-gl-js/pull/7483 ships.
 */
export function mltAcceptTransformRequest(
  getMap: () => MapLibre | undefined,
): RequestTransformFunction {
  return (url, resourceType) =>
    resourceType === 'Tile' && servesMlt(getMap(), url)
      ? { headers: { Accept: MLT_MIME }, url }
      : undefined;
}
