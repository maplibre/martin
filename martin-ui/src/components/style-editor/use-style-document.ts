import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { useEffect, useRef, useState } from 'react';

/** One entry of the edit history. */
export interface Edit {
  /** Unique within the session, for list keys. */
  id: number;
  /** What changed, shown in the history list. */
  label: string;
  /** Edits with the same key in a row merge into one entry, e.g. while dragging a slider. */
  coalesce?: string;
}

/** The style being edited plus its undo/redo history; nothing is persisted. */
export interface StyleDocument {
  /** Current style, `null` while loading. */
  style: StyleSpecification | null;
  /** Load failure, if any. */
  error: string | null;
  /** Applied edits, oldest first. */
  edits: Edit[];
  /** Whether `undo` has anything to revert. */
  canUndo: boolean;
  /** Whether `redo` has anything to reapply. */
  canRedo: boolean;
  /** Applies `recipe` to a copy of the current style and records it. */
  apply: (label: string, recipe: (draft: StyleSpecification) => void, coalesce?: string) => void;
  /** Reverts the last edit. */
  undo: () => void;
  /** Reapplies the last reverted edit. */
  redo: () => void;
  /** Drops every edit. */
  reset: () => void;
}

interface Entry {
  style: StyleSpecification;
  edit: Edit;
}

/** Loads the style at `url` and tracks edits to it in memory. */
export function useStyleDocument(url: string): StyleDocument {
  const [original, setOriginal] = useState<StyleSpecification | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<Entry[]>([]);
  const [undone, setUndone] = useState<Entry[]>([]);
  const nextId = useRef(0);

  useEffect(() => {
    const controller = new AbortController();
    fetch(url, { signal: controller.signal })
      .then((r) => (r.ok ? r.json() : Promise.reject(new Error(`${r.status} ${r.statusText}`))))
      .then((s: StyleSpecification) => setOriginal(s))
      .catch((e: Error) => {
        if (e.name !== 'AbortError') setError(e.message);
      });
    return () => controller.abort();
  }, [url]);

  const style = done.at(-1)?.style ?? original;

  return {
    apply: (label, recipe, coalesce) => {
      if (!style) return;
      const next = structuredClone(style);
      recipe(next);
      nextId.current += 1;
      const entry = { edit: { coalesce, id: nextId.current, label }, style: next };
      setDone((d) =>
        coalesce && d.at(-1)?.edit.coalesce === coalesce
          ? [...d.slice(0, -1), entry]
          : [...d, entry],
      );
      setUndone([]);
    },
    canRedo: undone.length > 0,
    canUndo: done.length > 0,
    edits: done.map((d) => d.edit),
    error,
    redo: () => {
      const last = undone.at(-1);
      if (!last) return;
      setUndone((u) => u.slice(0, -1));
      setDone((d) => [...d, last]);
    },
    reset: () => {
      setDone([]);
      setUndone([]);
    },
    style,
    undo: () => {
      const last = done.at(-1);
      if (!last) return;
      setDone((d) => d.slice(0, -1));
      setUndone((u) => [...u, { ...last, edit: { id: last.edit.id, label: last.edit.label } }]);
    },
  };
}
