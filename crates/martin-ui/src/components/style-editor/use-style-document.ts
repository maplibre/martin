import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { useEffect, useRef, useState } from 'react';

/** One entry of the edit history. */
export interface Edit {
  /** Unique within the session, for list keys. */
  id: number;
  /** What changed, shown in the history list. */
  label: string;
  /** Edits with the same key in a row merge into one entry while one interaction lasts, e.g. while dragging a slider. */
  coalesce?: string;
}

/** A past state of the style: what it looked like right after `edit`. */
export interface Version {
  /** The edit that produced this state. */
  edit: Edit;
  /** The style after the edit. */
  style: StyleSpecification;
}

/** The style being edited plus its undo/redo history; nothing is persisted. */
export interface StyleDocument {
  /** Current style, `null` while loading. */
  style: StyleSpecification | null;
  /** The style as loaded from the server, `null` while loading. */
  original: StyleSpecification | null;
  /** States after each applied edit, oldest first. */
  versions: readonly Version[];
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

/** Loads the style at `url` and tracks edits to it in memory; an interaction, which ends on pointer up or when focus leaves an element, merges edits that share a coalesce key. */
export function useStyleDocument(url: string): StyleDocument {
  const [original, setOriginal] = useState<StyleSpecification | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<Version[]>([]);
  const [undone, setUndone] = useState<Version[]>([]);
  const nextId = useRef(0);
  const interactionEnded = useRef(false);

  useEffect(() => {
    const end = () => {
      interactionEnded.current = true;
    };
    document.addEventListener('pointerup', end, true);
    document.addEventListener('focusout', end, true);
    return () => {
      document.removeEventListener('pointerup', end, true);
      document.removeEventListener('focusout', end, true);
    };
  }, []);

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
      const mayMerge = coalesce !== undefined && !interactionEnded.current;
      interactionEnded.current = false;
      setDone((d) =>
        mayMerge && d.at(-1)?.edit.coalesce === coalesce
          ? [...d.slice(0, -1), entry]
          : [...d, entry],
      );
      setUndone([]);
    },
    canRedo: undone.length > 0,
    canUndo: done.length > 0,
    edits: done.map((d) => d.edit),
    error,
    original,
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
    versions: done,
  };
}
