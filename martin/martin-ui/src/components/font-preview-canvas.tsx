import Pbf from 'pbf';
import { useEffect, useRef, useState } from 'react';
import { buildMartinUrl } from '@/lib/api';
import { Skeleton } from '@/components/ui/skeleton';

const PREVIEW_TEXT = 'The quick brown fox jumps over the lazy dog';
const GLYPH_PBF_BORDER = 3;
const SDF_EDGE = 192;
const SDF_GAMMA = 20;

interface Glyph {
  id: number;
  bitmap: Uint8Array;
  width: number;
  height: number;
  left: number;
  top: number;
  advance: number;
}

function isCompleteGlyph(g: Partial<Glyph>): g is Glyph {
  return g.id !== undefined
    && g.bitmap !== undefined
    && g.width !== undefined
    && g.height !== undefined
    && g.left !== undefined
    && g.top !== undefined
    && g.advance !== undefined;
}

function parseGlyphPbf(data: ArrayBuffer): Map<number, Glyph> {
  const glyphs = new Map<number, Glyph>();
  const pbf = new Pbf(data);

  pbf.readFields((tag: number, _result: unknown, pbf: Pbf) => {
    if (tag === 1) {
      pbf.readMessage((tag: number, _result: unknown, pbf: Pbf) => {
        if (tag === 3) {
          const g: Partial<Glyph> = {};
          pbf.readMessage((tag: number, _result: unknown, pbf: Pbf) => {
            switch (tag) {
              case 1: g.id = pbf.readVarint(); break;
              case 2: g.bitmap = pbf.readBytes(); break;
              case 3: g.width = pbf.readVarint(); break;
              case 4: g.height = pbf.readVarint(); break;
              case 5: g.left = pbf.readSVarint(); break;
              case 6: g.top = pbf.readSVarint(); break;
              case 7: g.advance = pbf.readVarint(); break;
            }
          }, null);
          if (isCompleteGlyph(g)) glyphs.set(g.id, g);
        }
      }, null);
    }
  }, null);

  return glyphs;
}

function renderGlyphs(canvas: HTMLCanvasElement, glyphs: Map<number, Glyph>, fontSize: number) {
  const ctx = canvas.getContext('2d');
  if (!ctx) return;

  const scale = fontSize / 24;
  const BORDER = GLYPH_PBF_BORDER;
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  canvas.width = w * dpr;
  canvas.height = h * dpr;
  ctx.scale(dpr, dpr);

  // First pass: compute bounding box for vertical centering.
  // In the PBF format, top = bearingY - ascender (negative), so
  // the bitmap y-offset from a reference line is (-top - BORDER).
  let totalWidth = 0;
  let minY = Infinity;
  let maxY = -Infinity;

  for (const ch of PREVIEW_TEXT) {
    const g = glyphs.get(ch.charCodeAt(0));
    if (!g) { totalWidth += 8 * scale; continue; }
    if (g.width > 0 && g.height > 0) {
      const yTop = (-g.top - BORDER) * scale;
      const yBottom = yTop + (g.height + 2 * BORDER) * scale;
      minY = Math.min(minY, yTop);
      maxY = Math.max(maxY, yBottom);
    }
    totalWidth += g.advance * scale;
  }

  if (minY === Infinity) return;

  const offsetY = (h - (maxY - minY)) / 2 - minY;
  let x = Math.max(4, (w - totalWidth) / 2);

  // Reuse a single offscreen canvas for all glyph blits
  const tmp = document.createElement('canvas');
  const tmpCtx = tmp.getContext('2d');
  if (!tmpCtx) return;

  // Second pass: render glyphs
  for (const ch of PREVIEW_TEXT) {
    const g = glyphs.get(ch.charCodeAt(0));
    if (!g) { x += 8 * scale; continue; }

    if (g.width > 0 && g.height > 0) {
      const bw = g.width + 2 * BORDER;
      const bh = g.height + 2 * BORDER;
      const imgData = ctx.createImageData(bw, bh);

      for (let i = 0; i < g.bitmap.length; i++) {
        const a = Math.max(0, Math.min(255,
          ((g.bitmap[i] - SDF_EDGE + SDF_GAMMA) / (2 * SDF_GAMMA)) * 255,
        ));
        const j = i * 4;
        imgData.data[j] = 0x11;
        imgData.data[j + 1] = 0x18;
        imgData.data[j + 2] = 0x27;
        imgData.data[j + 3] = a;
      }

      tmp.width = bw;
      tmp.height = bh;
      tmpCtx.putImageData(imgData, 0, 0);

      ctx.drawImage(
        tmp,
        x + (g.left - BORDER) * scale,
        offsetY + (-g.top - BORDER) * scale,
        bw * scale,
        bh * scale,
      );
    }
    x += g.advance * scale;
  }
}

interface FontPreviewCanvasProps {
  fontName: string;
}

export function FontPreviewCanvas({ fontName }: FontPreviewCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [error, setError] = useState(false);
  const [glyphs, setGlyphs] = useState<Map<number, Glyph> | null>(null);

  // Fetch and parse glyphs
  useEffect(() => {
    let cancelled = false;
    setGlyphs(null);
    setError(false);

    fetch(buildMartinUrl(`/font/${fontName}/0-255`))
      .then((r) => {
        if (!r.ok) throw new Error(r.statusText);
        return r.arrayBuffer();
      })
      .then((buf) => {
        if (!cancelled) setGlyphs(parseGlyphPbf(buf));
      })
      .catch(() => {
        if (!cancelled) setError(true);
      });

    return () => { cancelled = true; };
  }, [fontName]);

  // Render once glyphs are loaded and canvas is visible; re-render on resize
  useEffect(() => {
    if (!glyphs || !canvasRef.current) return;
    const canvas = canvasRef.current;
    renderGlyphs(canvas, glyphs, 20);

    const observer = new ResizeObserver(() => renderGlyphs(canvas, glyphs, 20));
    observer.observe(canvas);
    return () => observer.disconnect();
  }, [glyphs]);

  if (error) {
    return <p className="text-sm text-muted-foreground italic">Preview unavailable</p>;
  }

  if (!glyphs) {
    return <Skeleton className="w-full h-12" />;
  }

  return <canvas ref={canvasRef} className="w-full h-12" />;
}
