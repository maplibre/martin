import { useTheme } from 'next-themes';
import { useEffect, useRef, useState } from 'react';
import { BORDER, type Glyph, parseGlyphs } from '@/lib/glyphs';

const SAMPLE = 'Amsterdam 123';
const SCALE = 0.75;
const glyphCache = new Map<string, Promise<Map<number, Glyph> | null>>();

function loadGlyphs(url: string): Promise<Map<number, Glyph> | null> {
  const cached = glyphCache.get(url);
  if (cached) return cached;
  const pending = fetch(url)
    .then((r) => {
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      return r.arrayBuffer();
    })
    .then((buf) => {
      const glyphs = parseGlyphs(buf);
      return glyphs.size ? glyphs : null;
    })
    .catch(() => null);
  glyphCache.set(url, pending);
  // A failure is not cached so a later open can retry.
  void pending.then((g) => {
    if (!g) glyphCache.delete(url);
  });
  return pending;
}

function draw(canvas: HTMLCanvasElement, glyphs: Map<number, Glyph>) {
  const ctx = canvas.getContext('2d');
  const tmp = document.createElement('canvas');
  const tc = tmp.getContext('2d');
  if (!ctx || !tc) return;
  let minY = Infinity;
  let maxY = -Infinity;
  let width = 4;
  for (const c of SAMPLE) {
    const g = glyphs.get(c.charCodeAt(0));
    width += (g?.advance ?? 8) * SCALE;
    if (g?.width) {
      minY = Math.min(minY, (-g.top - BORDER) * SCALE);
      maxY = Math.max(maxY, (-g.top + g.height + BORDER) * SCALE);
    }
  }
  if (!Number.isFinite(minY)) return;
  const w = Math.ceil(width);
  const h = Math.ceil(maxY - minY) + 2;
  const dpr = window.devicePixelRatio || 1;
  canvas.width = w * dpr;
  canvas.height = h * dpr;
  canvas.style.width = `${w}px`;
  canvas.style.height = `${h}px`;
  ctx.scale(dpr, dpr);
  const baseline = 1 - minY;
  let x = 2;
  for (const c of SAMPLE) {
    const g = glyphs.get(c.charCodeAt(0));
    if (!g) {
      x += 8 * SCALE;
      continue;
    }
    if (g.width > 0 && g.height > 0) {
      const bw = g.width + 2 * BORDER;
      const bh = g.height + 2 * BORDER;
      const img = ctx.createImageData(bw, bh);
      for (let k = 0; k < g.bitmap.length; k++) {
        // Distance field: 192 is the outline, so a narrow ramp around it gives a crisp edge.
        const a = Math.max(0, Math.min(255, ((g.bitmap[k] - 172) / 40) * 255));
        img.data.set([0, 0, 0, a], k * 4);
      }
      tmp.width = bw;
      tmp.height = bh;
      tc.putImageData(img, 0, 0);
      ctx.drawImage(
        tmp,
        x + (g.left - BORDER) * SCALE,
        baseline + (-g.top - BORDER) * SCALE,
        bw * SCALE,
        bh * SCALE,
      );
    }
    x += g.advance * SCALE;
  }
  ctx.globalCompositeOperation = 'source-in';
  ctx.fillStyle = getComputedStyle(canvas).color;
  ctx.fillRect(0, 0, w, h);
  ctx.globalCompositeOperation = 'source-over';
}

/** Draws "Amsterdam 123" in the given font from the style's glyph server once scrolled into view. */
export function GlyphSample({ name, glyphsUrl }: { name: string; glyphsUrl: string | undefined }) {
  const wrapRef = useRef<HTMLSpanElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [visible, setVisible] = useState(typeof IntersectionObserver === 'undefined');
  const [glyphs, setGlyphs] = useState<Map<number, Glyph> | null | undefined>(undefined);
  const { resolvedTheme } = useTheme();

  useEffect(() => {
    const el = wrapRef.current;
    if (visible || !el) return;
    let obs: IntersectionObserver;
    try {
      obs = new IntersectionObserver((entries) => {
        if (entries.some((e) => e.isIntersecting)) setVisible(true);
      });
    } catch {
      setVisible(true);
      return;
    }
    obs.observe(el);
    return () => obs.disconnect();
  }, [visible]);

  useEffect(() => {
    if (!visible) return;
    if (!glyphsUrl) {
      setGlyphs(null);
      return;
    }
    let cancelled = false;
    const url = glyphsUrl
      .replace('{fontstack}', encodeURIComponent(name))
      .replace('{range}', '0-255');
    void loadGlyphs(url).then((g) => {
      if (!cancelled) setGlyphs(g);
    });
    return () => {
      cancelled = true;
    };
  }, [visible, glyphsUrl, name]);

  useEffect(() => {
    if (glyphs && canvasRef.current) draw(canvasRef.current, glyphs);
  }, [glyphs, resolvedTheme]);

  return (
    <span className="flex h-5 items-center" ref={wrapRef}>
      {glyphs ? (
        <canvas
          aria-label={`Preview of ${name}`}
          className="block text-foreground"
          ref={canvasRef}
          role="img"
        />
      ) : glyphs === null ? (
        <span className="text-[11px] text-muted-foreground italic">no preview</span>
      ) : null}
    </span>
  );
}
