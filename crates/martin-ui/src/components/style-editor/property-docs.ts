import { useSyncExternalStore } from 'react';
import type { LayerType, PropertyGroup } from './style-spec';

type DocTable = Record<string, Record<string, { doc?: string } | undefined> | undefined>;

let table: DocTable | null = null;
let loading = false;
const listeners = new Set<() => void>();

// The runtime `latest` export strips every `doc`; the docs only exist in the full reference JSON, loaded on demand.
function load() {
  if (loading) return;
  loading = true;
  import('@maplibre/maplibre-gl-style-spec/dist/latest.json')
    .then((m: { default: unknown }) => {
      table = m.default as DocTable;
      for (const notify of listeners) notify();
    })
    .catch(() => {
      loading = false;
    });
}

function subscribe(notify: () => void) {
  listeners.add(notify);
  load();
  return () => {
    listeners.delete(notify);
  };
}

/** Looks up the v8 documentation of a property; returns `undefined` until the reference has loaded. */
export function usePropertyDocs(): (
  type: LayerType,
  group: PropertyGroup,
  name: string,
) => string | undefined {
  const docs = useSyncExternalStore(subscribe, () => table);
  return (type, group, name) => docs?.[`${group}_${type}`]?.[name]?.doc;
}
