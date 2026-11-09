import { PbfReader } from 'pbf';

type Ring = readonly (readonly [number, number])[];

export interface PreviewFeature {
  type: 1 | 2 | 3;
  rings: Ring[];
}

export interface PreviewLayer {
  name: string;
  extent: number;
  features: PreviewFeature[];
}

interface RawLayer {
  name: string;
  extent: number;
  features: PreviewFeature[];
}

function readGeometry(pbf: PbfReader, end: number, type: 1 | 2 | 3): Ring[] {
  const rings: [number, number][][] = [];
  let ring: [number, number][] = [];
  let x = 0;
  let y = 0;
  while (pbf.pos < end) {
    const header = pbf.readVarint();
    const command = header & 7;
    let count = header >> 3;
    if (command === 7) {
      if (ring.length > 0 && type === 3) ring.push(ring[0]);
      continue;
    }
    while (count-- > 0) {
      x += pbf.readSVarint();
      y += pbf.readSVarint();
      if (command === 1) {
        if (type !== 1 && ring.length > 0) rings.push(ring);
        ring = [[x, y]];
        if (type === 1) {
          rings.push(ring);
          ring = [];
        }
      } else {
        ring.push([x, y]);
      }
    }
  }
  if (type !== 1 && ring.length > 0) rings.push(ring);
  return rings;
}

function readFeature(pbf: PbfReader, end: number, layer: RawLayer) {
  let type: 0 | 1 | 2 | 3 = 0;
  let geometryStart = -1;
  let geometryEnd = -1;
  while (pbf.pos < end) {
    const val = pbf.readVarint();
    const tag = val >> 3;
    if (tag === 3) {
      type = pbf.readVarint() as 0 | 1 | 2 | 3;
    } else if (tag === 4) {
      const len = pbf.readVarint();
      geometryStart = pbf.pos;
      geometryEnd = pbf.pos + len;
      pbf.pos = geometryEnd;
    } else {
      pbf.skip(val & 7);
    }
  }
  if (type === 0 || geometryStart < 0) return;
  pbf.pos = geometryStart;
  layer.features.push({ rings: readGeometry(pbf, geometryEnd, type), type });
  pbf.pos = end;
}

/** Decode only the geometry of a Mapbox Vector Tile: enough to draw a thumbnail. */
export function decodeVectorTile(buffer: ArrayBuffer): PreviewLayer[] {
  const pbf = new PbfReader(new Uint8Array(buffer));
  const layers: PreviewLayer[] = [];
  pbf.readFields(
    (tag: number, acc: PreviewLayer[]) => {
      if (tag !== 3) return;
      const layer: RawLayer = { extent: 4096, features: [], name: '' };
      pbf.readMessage(
        (layerTag: number, l: RawLayer) => {
          if (layerTag === 1) l.name = pbf.readString();
          else if (layerTag === 5) l.extent = pbf.readVarint();
          else if (layerTag === 2) {
            const len = pbf.readVarint();
            readFeature(pbf, pbf.pos + len, l);
          }
        },
        layer,
      );
      acc.push(layer);
    },
    layers,
  );
  return layers;
}

export interface PreviewPalette {
  fill: string;
  line: string;
  point: string;
  halo: string;
}

/** Draw decoded layers into a square canvas context. */
export function drawVectorTile(
  ctx: CanvasRenderingContext2D,
  layers: PreviewLayer[],
  size: number,
  palette: PreviewPalette,
): number {
  let drawn = 0;
  for (const layer of layers) {
    const k = size / layer.extent;
    for (const feature of layer.features) {
      if (feature.type === 3) {
        ctx.beginPath();
        for (const ring of feature.rings) {
          ring.forEach(([x, y], i) => {
            if (i === 0) ctx.moveTo(x * k, y * k);
            else ctx.lineTo(x * k, y * k);
          });
          ctx.closePath();
        }
        ctx.fillStyle = palette.fill;
        ctx.fill('evenodd');
        ctx.strokeStyle = palette.line;
        ctx.lineWidth = 0.75;
        ctx.stroke();
        drawn++;
      } else if (feature.type === 2) {
        ctx.beginPath();
        for (const ring of feature.rings) {
          ring.forEach(([x, y], i) => {
            if (i === 0) ctx.moveTo(x * k, y * k);
            else ctx.lineTo(x * k, y * k);
          });
        }
        ctx.strokeStyle = palette.line;
        ctx.lineWidth = 1.25;
        ctx.lineJoin = 'round';
        ctx.stroke();
        drawn++;
      } else {
        for (const ring of feature.rings) {
          for (const [x, y] of ring) {
            ctx.beginPath();
            ctx.arc(x * k, y * k, 2.4, 0, Math.PI * 2);
            ctx.fillStyle = palette.point;
            ctx.fill();
            ctx.strokeStyle = palette.halo;
            ctx.lineWidth = 1;
            ctx.stroke();
            drawn++;
          }
        }
      }
    }
  }
  return drawn;
}
