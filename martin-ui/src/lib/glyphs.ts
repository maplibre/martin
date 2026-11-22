import { PbfReader } from 'pbf';

/** Width in pixels of the border an SDF glyph bitmap carries around its outline. */
export const BORDER = 3;

/** One glyph of an SDF glyph PBF: its distance-field bitmap and metrics. */
export type Glyph = {
  bitmap: Uint8Array;
  width: number;
  height: number;
  left: number;
  top: number;
  advance: number;
};

/** Decodes a glyph PBF range into its glyphs keyed by code point. */
export function parseGlyphs(data: ArrayBuffer) {
  const glyphs = new Map<number, Glyph>();
  new PbfReader(data).readFields((tag, _, pbf) => {
    if (tag === 1)
      pbf.readMessage((tag, _, pbf) => {
        if (tag === 3) {
          const g: Record<string, unknown> = {};
          pbf.readMessage((tag, _, pbf) => {
            if (tag === 1) g.id = pbf.readVarint();
            else if (tag === 2) g.bitmap = pbf.readBytes();
            else if (tag === 3) g.width = pbf.readVarint();
            else if (tag === 4) g.height = pbf.readVarint();
            else if (tag === 5) g.left = pbf.readSVarint();
            else if (tag === 6) g.top = pbf.readSVarint();
            else if (tag === 7) g.advance = pbf.readVarint();
          }, null);
          if (g.id != null && g.bitmap) glyphs.set(g.id as number, g as unknown as Glyph);
        }
      }, null);
  }, null);
  return glyphs;
}
