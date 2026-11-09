import { PbfReader } from 'pbf';
import { useTheme } from 'next-themes';
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

function render(canvas: HTMLCanvasElement, glyphs: Map<number, Glyph>) {
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  const s = 22 / 24;
  const dpr = devicePixelRatio || 1;
  const w = canvas.clientWidth;
  const advanceOf = (c: string) => (glyphs.get(c.charCodeAt(0))?.advance ?? 8) * s;

  // Vertical extent of the text, so every line shares the same box
  let minY = Infinity,
    maxY = -Infinity;
  for (const c of PREVIEW) {
    const g = glyphs.get(c.charCodeAt(0));
    if (g?.width) {
      const y = (-g.top - BORDER) * s;
      minY = Math.min(minY, y);
      maxY = Math.max(maxY, y + (g.height + 2 * BORDER) * s);
    }
  }
  if (!Number.isFinite(minY)) return;
  const lineHeight = Math.ceil(maxY - minY) + 6;

  // Greedy word wrap so the specimen never gets cut mid-word
  const lines: string[] = [];
  let line = '';
  for (const word of PREVIEW.split(' ')) {
    const candidate = line ? `${line} ${word}` : word;
    const width = [...candidate].reduce((sum, c) => sum + advanceOf(c), 0);
    if (line && width > w - 8) {
      lines.push(line);
      line = word;
    } else {
      line = candidate;
    }
  }
  if (line) lines.push(line);

  const h = lines.length * lineHeight + 4;
  canvas.style.height = `${h}px`;
  canvas.width = w * dpr;
  canvas.height = h * dpr;
  ctx.scale(dpr, dpr);

  const tmp = document.createElement('canvas');
  const tc = tmp.getContext('2d');
  if (!tc) return;

  lines.forEach((text, i) => {
    const oy = i * lineHeight + 2 - minY + 3;
    let x = 4;
    for (const c of text) {
      const g = glyphs.get(c.charCodeAt(0));
      if (!g) {
        x += 8 * s;
        continue;
      }
      if (g.width > 0 && g.height > 0) {
        const bw = g.width + 6,
          bh = g.height + 6;
        const id = ctx.createImageData(bw, bh);
        for (let k = 0; k < g.bitmap.length; k++) {
          const a = Math.max(0, Math.min(255, ((g.bitmap[k] - 172) / 40) * 255));
          id.data.set([0, 0, 0, a], k * 4);
        }
        tmp.width = bw;
        tmp.height = bh;
        tc.putImageData(id, 0, 0);
        ctx.drawImage(tmp, x + (g.left - BORDER) * s, oy + (-g.top - BORDER) * s, bw * s, bh * s);
      }
      x += g.advance * s;
    }
  });

  // Tint the glyph coverage with the current text colour so it follows the theme
  ctx.globalCompositeOperation = 'source-in';
  ctx.fillStyle = getComputedStyle(canvas).color;
  ctx.fillRect(0, 0, w, h);
  ctx.globalCompositeOperation = 'source-over';
}

export function FontPreviewCanvas({ fontName }: { fontName: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [error, setError] = useState(false);
  const [glyphs, setGlyphs] = useState<Map<number, Glyph> | null>(null);
  const { resolvedTheme } = useTheme();

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
  }, [glyphs, resolvedTheme]);

  if (error) return <p className="text-sm text-muted-foreground italic">Preview unavailable</p>;
  if (!glyphs) return <Skeleton className="h-14 w-full" />;
  return <canvas aria-label={`Preview of ${fontName}`} className="block h-14 w-full" ref={ref} role="img" />;
}
