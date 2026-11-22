import { Plus, Search } from 'lucide-react';
import { useState } from 'react';
import { usePropertyDocs } from '../property-docs';
import type { LayerType, PropertyEntry } from '../style-spec';
import { Popover } from './popover';

/** Props of the add-property control. */
export interface AddPropertyProps {
  /** Layer type the properties belong to, for looking up their docs. */
  layerType: LayerType;
  /** Properties that are not set yet. */
  entries: PropertyEntry[];
  /** Called with the property to add at its default. */
  onAdd: (entry: PropertyEntry) => void;
}

/** Searchable popover listing unset properties with their documentation. */
export function AddProperty({ layerType, entries, onAdd }: AddPropertyProps) {
  const [query, setQuery] = useState('');
  const docOf = usePropertyDocs();
  const doc = (e: PropertyEntry) => docOf(layerType, e.group, e.name) ?? '';
  const q = query.trim().toLowerCase();
  const shown = entries.filter((e) => e.name.includes(q) || doc(e).toLowerCase().includes(q));
  return (
    <Popover
      panelClassName="left-0 right-0 top-full mt-1"
      trigger={({ open, toggle }) => (
        <button
          aria-expanded={open}
          className="flex h-7 w-full items-center justify-center gap-1.5 rounded-md border border-dashed text-xs text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
          onClick={() => {
            setQuery('');
            toggle();
          }}
          type="button"
        >
          <Plus className="size-3.5" />
          Add property
        </button>
      )}
    >
      {(close) => (
        <div>
          <div className="flex items-center gap-2 border-b px-2">
            <Search className="size-3.5 shrink-0 text-muted-foreground" />
            <input
              className="h-8 min-w-0 flex-1 bg-transparent text-xs focus-visible:outline-none"
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search properties"
              ref={(el) => el?.focus()}
              value={query}
            />
          </div>
          <div className="max-h-72 overflow-y-auto p-1">
            {shown.length === 0 && (
              <p className="px-2 py-3 text-center text-[11px] text-muted-foreground">No match</p>
            )}
            {shown.map((e) => (
              <button
                className="block w-full rounded-sm px-2 py-1.5 text-left hover:bg-muted"
                key={`${e.group}-${e.name}`}
                onClick={() => {
                  close();
                  onAdd(e);
                }}
                type="button"
              >
                <span className="block truncate font-mono text-xs" title={e.name}>
                  {e.name}
                </span>
                <span className="line-clamp-2 text-[11px] text-muted-foreground">{doc(e)}</span>
              </button>
            ))}
          </div>
        </div>
      )}
    </Popover>
  );
}
