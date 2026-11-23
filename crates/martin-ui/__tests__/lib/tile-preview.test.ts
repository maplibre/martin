import { PbfWriter } from 'pbf';
import { describe, expect, it } from 'vitest';
import { decodeVectorTile } from '@/lib/tile-preview';

const zigzag = (n: number) => (n << 1) ^ (n >> 31);

function encodeTile(geomType: number, geometry: number[]): ArrayBuffer {
  const feature = new PbfWriter();
  feature.writeVarintField(3, geomType);
  feature.writePackedVarint(4, geometry);
  const layer = new PbfWriter();
  layer.writeStringField(1, 'cities');
  layer.writeBytesField(2, feature.finish());
  layer.writeVarintField(5, 4096);
  const tile = new PbfWriter();
  tile.writeBytesField(3, layer.finish());
  const bytes = tile.finish();
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
}

describe('decodeVectorTile', () => {
  it('decodes a point', () => {
    const layers = decodeVectorTile(encodeTile(1, [9, zigzag(25), zigzag(17)]));
    expect(layers).toHaveLength(1);
    expect(layers[0].name).toBe('cities');
    expect(layers[0].extent).toBe(4096);
    expect(layers[0].features[0].type).toBe(1);
    expect(layers[0].features[0].rings).toEqual([[[25, 17]]]);
  });

  it('closes polygon rings', () => {
    // MoveTo(0,0) LineTo(10,0) LineTo(0,10) ClosePath
    const geometry = [9, 0, 0, 18, zigzag(10), 0, zigzag(-10), zigzag(10), 15];
    const [feature] = decodeVectorTile(encodeTile(3, geometry))[0].features;
    expect(feature.type).toBe(3);
    expect(feature.rings[0]).toEqual([
      [0, 0],
      [10, 0],
      [0, 10],
      [0, 0],
    ]);
  });
});
