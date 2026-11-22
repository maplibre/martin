import { useEffect, useState } from 'react';

/** Outcome of a probe request. */
export type ProbeState =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'ok'; text: string }
  | { status: 'error'; text: string };

const cache = new Map<string, ProbeState>();

class ProbeHttpError extends Error {
  constructor(readonly status: number) {
    super(`HTTP ${status}`);
  }
}

/** Describes a failed fetch the way the status lines show it, e.g. `Failed: 404`. */
export function failure(error: unknown): string {
  if (error instanceof ProbeHttpError) return `Failed: ${error.status}`;
  return `Failed: ${error instanceof Error ? error.message : 'unreachable'}`;
}

/** Fetches `url`, throwing on a non-2xx status. */
export async function fetchChecked(url: string, signal: AbortSignal): Promise<Response> {
  const response = await fetch(url, { signal });
  if (!response.ok) throw new ProbeHttpError(response.status);
  return response;
}

/** Fetches `url` and returns its body as JSON, throwing on a non-2xx status. */
export async function fetchJson(url: string, signal: AbortSignal): Promise<unknown> {
  return (await fetchChecked(url, signal)).json();
}

/** Runs `run` for `key` after a short pause and caches the result; `key` undefined means nothing to probe. */
export function useProbe(
  key: string | undefined,
  run: (signal: AbortSignal) => Promise<string>,
  delayMs = 400,
): ProbeState {
  const [result, setResult] = useState<{ key: string; state: ProbeState }>();
  useEffect(() => {
    if (key === undefined || cache.has(key)) return;
    const controller = new AbortController();
    const timer = setTimeout(() => {
      run(controller.signal)
        .then((text): ProbeState => ({ status: 'ok', text }))
        .catch((e: unknown): ProbeState | undefined =>
          controller.signal.aborted ? undefined : { status: 'error', text: failure(e) },
        )
        .then((state) => {
          if (!state) return;
          cache.set(key, state);
          setResult({ key, state });
        });
    }, delayMs);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [key, delayMs]);
  if (key === undefined) return { status: 'idle' };
  const cached = cache.get(key);
  if (cached) return cached;
  return result?.key === key ? result.state : { status: 'loading' };
}

/** One `{id, url}` icon sheet of a style's `sprite`. */
export interface SpriteSheet {
  /** Prefix of the sheet's icon names; `default` adds none. */
  id: string;
  /** Base URL, extended with `.json` and `.png`. */
  url: string;
}

/** The sheets a `sprite` value defines; the string form is the single sheet `default`. */
export function spriteSheets(sprite: string | SpriteSheet[] | undefined): SpriteSheet[] {
  if (sprite === undefined) return [];
  return typeof sprite === 'string' ? [{ id: 'default', url: sprite }] : sprite;
}

/** The `sprite` value for these sheets: the string form for one `default` sheet, `undefined` for none. */
export function spriteValue(sheets: SpriteSheet[]): string | SpriteSheet[] | undefined {
  if (sheets.length === 0) return undefined;
  if (sheets.length === 1 && sheets[0].id === 'default') return sheets[0].url;
  return sheets;
}

/** Names of the placeholders a `glyphs` template still lacks. */
export function missingGlyphTokens(template: string): string[] {
  return ['{fontstack}', '{range}'].filter((t) => !template.includes(t));
}

/** The URL a `glyphs` template resolves to for one font and code point range. */
export function glyphUrl(template: string, font: string, range: string): string {
  return template.replace('{fontstack}', encodeURIComponent(font)).replace('{range}', range);
}
