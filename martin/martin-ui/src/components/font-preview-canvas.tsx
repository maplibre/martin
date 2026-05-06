import Pbf from 'pbf';
import { useEffect, useRef, useState } from 'react';
import { Skeleton } from '@/components/ui/skeleton';
import { buildMartinUrl } from '@/lib/api';

const PREVIEW = 'The quick brown fox jumps over the lazy dog';
const BORDER = 3; // SDF glyph PBF border size

type Glyph = {
  bitmap: Uint8Array;
  width: number;
  height: number;
  left: number;
  top: number;
  advance: number;
};

function parseGlyphs(data: ArrayBuffer) {
  const glyphs = new Map<number, Glyph>();
  new Pbf(data).readFields((tag, _, pbf) => {
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

function render(canvas: HTMLCanvasElement, glyphs: Map<number, Glyph>) {
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  const s = 20 / 24;
  const dpr = devicePixelRatio || 1;
  const w = canvas.clientWidth,
    h = canvas.clientHeight;
  canvas.width = w * dpr;
  canvas.height = h * dpr;
  ctx.scale(dpr, dpr);

  // Measure bounding box for centering
  let tw = 0,
    minY = Infinity,
    maxY = -Infinity;
  for (const c of PREVIEW) {
    const g = glyphs.get(c.charCodeAt(0));
    if (g?.width) {
      const y = (-g.top - BORDER) * s;
      minY = Math.min(minY, y);
      maxY = Math.max(maxY, y + (g.height + 2 * BORDER) * s);
    }
    tw += (g?.advance ?? 8) * s;
  }
  if (!Number.isFinite(minY)) return;

  const oy = (h - maxY + minY) / 2 - minY;
  let x = Math.max(4, (w - tw) / 2);
  const tmp = document.createElement('canvas');
  const tc = tmp.getContext('2d');
  if (!tc) return;

  // Render each glyph's SDF bitmap
  for (const c of PREVIEW) {
    const g = glyphs.get(c.charCodeAt(0));
    if (!g) {
      x += 8 * s;
      continue;
    }
    if (g.width > 0 && g.height > 0) {
      const bw = g.width + 6,
        bh = g.height + 6;
      const id = ctx.createImageData(bw, bh);
      for (let i = 0; i < g.bitmap.length; i++) {
        const a = Math.max(0, Math.min(255, ((g.bitmap[i] - 172) / 40) * 255));
        id.data.set([0x11, 0x18, 0x27, a], i * 4);
      }
      tmp.width = bw;
      tmp.height = bh;
      tc.putImageData(id, 0, 0);
      ctx.drawImage(tmp, x + (g.left - BORDER) * s, oy + (-g.top - BORDER) * s, bw * s, bh * s);
    }
    x += g.advance * s;
  }
}

export function FontPreviewCanvas({ fontName }: { fontName: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [error, setError] = useState(false);
  const [glyphs, setGlyphs] = useState<Map<number, Glyph> | null>(null);

  useEffect(() => {
    let cancelled = false;
    setGlyphs(null);
    setError(false);
    fetch(buildMartinUrl(`/font/${fontName}/0-255`))
      .then((r) => {
        if (!r.ok) throw r;
        return r.arrayBuffer();
      })
      .then((buf) => {
        if (!cancelled) setGlyphs(parseGlyphs(buf));
      })
      .catch(() => {
        if (!cancelled) setError(true);
      });
    return () => {
      cancelled = true;
    };
  }, [fontName]);

  useEffect(() => {
    if (!glyphs || !ref.current) return;
    const canvas = ref.current;
    render(canvas, glyphs);
    const obs = new ResizeObserver(() => render(canvas, glyphs));
    obs.observe(canvas);
    return () => obs.disconnect();
  }, [glyphs]);

  if (error) return <p className="text-sm text-muted-foreground italic">Preview unavailable</p>;
  if (!glyphs) return <Skeleton className="w-full h-12" />;
  return <canvas className="w-full h-12" ref={ref} />;
}
