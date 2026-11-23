import type { Map as MapLibre, ResourceType, VectorTileSource } from 'maplibre-gl';
import { describe, expect, it } from 'vitest';
import { mltAcceptTransformRequest } from '@/lib/mlt';

const TILE: ResourceType = 'Tile' as ResourceType;
const SOURCE: ResourceType = 'Source' as ResourceType;

function fakeMap(sources: Record<string, Partial<VectorTileSource>>, isStyleLoaded = true) {
  return {
    getSource: (id: string) => sources[id],
    getStyle: () => ({ sources }),
    isStyleLoaded: () => isStyleLoaded,
  } as unknown as MapLibre;
}

const mltSource: Partial<VectorTileSource> = {
  encoding: 'mlt',
  tiles: ['http://localhost:3000/mlt_source/{z}/{x}/{y}'],
  type: 'vector',
};

const mvtSource: Partial<VectorTileSource> = {
  encoding: 'mvt',
  tiles: ['http://localhost:3000/mvt_source/{z}/{x}/{y}'],
  type: 'vector',
};

describe('mltAcceptTransformRequest', () => {
  it('adds the Accept header for a tile of an mlt-encoded source', () => {
    const transform = mltAcceptTransformRequest(() => fakeMap({ mlt_source: mltSource }));

    expect(transform('http://localhost:3000/mlt_source/0/0/0', TILE)).toEqual({
      headers: { Accept: 'application/vnd.maplibre-tile' },
      url: 'http://localhost:3000/mlt_source/0/0/0',
    });
  });

  it('leaves non-tile resources alone', () => {
    const transform = mltAcceptTransformRequest(() => fakeMap({ mlt_source: mltSource }));

    expect(transform('http://localhost:3000/mlt_source', SOURCE)).toBeUndefined();
    expect(transform('http://localhost:3000/mlt_source/0/0/0', undefined)).toBeUndefined();
  });

  it('leaves mvt-encoded sources alone', () => {
    const transform = mltAcceptTransformRequest(() => fakeMap({ mvt_source: mvtSource }));

    expect(transform('http://localhost:3000/mvt_source/0/0/0', TILE)).toBeUndefined();
  });

  it('leaves tiles of a different source alone', () => {
    const transform = mltAcceptTransformRequest(() =>
      fakeMap({ mlt_source: mltSource, mvt_source: mvtSource }),
    );

    expect(transform('http://localhost:3000/mvt_source/0/0/0', TILE)).toBeUndefined();
    expect(transform('http://localhost:3000/mlt_source/0/0/0', TILE)).toEqual({
      headers: { Accept: 'application/vnd.maplibre-tile' },
      url: 'http://localhost:3000/mlt_source/0/0/0',
    });
  });

  it('does nothing without a loaded map', () => {
    expect(mltAcceptTransformRequest(() => undefined)('http://localhost:3000/x/0/0/0', TILE)).toBe(
      undefined,
    );
    expect(
      mltAcceptTransformRequest(() => fakeMap({ mlt_source: mltSource }, false))(
        'http://localhost:3000/mlt_source/0/0/0',
        TILE,
      ),
    ).toBeUndefined();
  });
});
