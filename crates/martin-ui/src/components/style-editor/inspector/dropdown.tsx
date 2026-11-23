import { Check, ChevronDown } from 'lucide-react';
import { type KeyboardEvent, useState } from 'react';
import { cn } from '@/lib/utils';
import { Popover } from './popover';

/** One choice of a `Dropdown`. */
export interface DropdownOption {
  /** Value passed to `onChange`, also the displayed text. */
  value: string;
  /** Small badge shown after the value, e.g. a source type. */
  badge?: string;
}

/** Props of the compact dropdown field. */
export interface DropdownProps {
  /** Accessible name of the field. */
  label: string;
  /** Currently selected value; may be absent from `options`. */
  value: string | undefined;
  /** Choices in display order. */
  options: DropdownOption[];
  /** Called with the chosen or typed value. */
  onChange: (value: string) => void;
  /** Lets Enter in the search box pick the typed text when it is not an option. */
  allowCustom?: boolean;
  /** Shown in the trigger when `value` is unset. */
  placeholder?: string;
}

const SEARCH_THRESHOLD = 8;

function Panel({
  options,
  value,
  onChange,
  allowCustom,
  label,
}: Pick<DropdownProps, 'options' | 'value' | 'onChange' | 'allowCustom' | 'label'>) {
  const [query, setQuery] = useState('');
  const searchable = allowCustom === true || options.length > SEARCH_THRESHOLD;
  const needle = query.trim().toLowerCase();
  const shown = options.filter((o) => o.value.toLowerCase().includes(needle));
  const custom = allowCustom && needle !== '' && !options.some((o) => o.value === query.trim());
  const choices = custom ? [{ badge: 'custom', value: query.trim() }, ...shown] : shown;
  const [cursor, setCursor] = useState(() =>
    Math.max(
      0,
      options.findIndex((o) => o.value === value),
    ),
  );
  const active = Math.min(cursor, Math.max(0, choices.length - 1));

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'ArrowDown') setCursor(Math.min(choices.length - 1, active + 1));
    else if (e.key === 'ArrowUp') setCursor(Math.max(0, active - 1));
    else if (e.key === 'Home') setCursor(0);
    else if (e.key === 'End') setCursor(choices.length - 1);
    else if (e.key === 'Enter' && choices[active]) onChange(choices[active].value);
    else return;
    e.preventDefault();
  };

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: key events bubble from the focused search box or listbox
    <div onKeyDown={onKeyDown}>
      {searchable ? (
        <input
          aria-label={`Search ${label.toLowerCase()}`}
          className="h-7 w-full border-b bg-transparent px-2 text-xs focus-visible:outline-none"
          onChange={(e) => {
            setQuery(e.target.value);
            setCursor(0);
          }}
          placeholder={allowCustom ? 'Search or type a name' : 'Search'}
          ref={(el) => el?.focus()}
          value={query}
        />
      ) : null}
      <div
        aria-label={label}
        className="max-h-56 overflow-y-auto p-1 outline-none"
        ref={(el) => {
          if (!searchable) el?.focus();
        }}
        role="listbox"
        tabIndex={searchable ? -1 : 0}
      >
        {choices.length === 0 ? (
          <div className="px-2 py-1.5 text-[11px] text-muted-foreground">No matches</div>
        ) : null}
        {choices.map((o, i) => (
          <button
            aria-selected={o.value === value}
            className={cn(
              'flex w-full min-w-0 cursor-pointer items-center text-left gap-1.5 rounded-sm px-2 py-1 text-xs',
              i === active && 'bg-muted',
            )}
            key={o.value}
            onClick={() => onChange(o.value)}
            onPointerMove={() => setCursor(i)}
            ref={(el) => {
              if (i === active) el?.scrollIntoView?.({ block: 'nearest' });
            }}
            role="option"
            tabIndex={-1}
            title={o.value}
            type="button"
          >
            <Check
              className={cn('size-3 shrink-0 text-editor-accent', o.value !== value && 'invisible')}
            />
            <span className="min-w-0 flex-1 truncate font-mono">{o.value}</span>
            {o.badge ? <Badge text={o.badge} /> : null}
          </button>
        ))}
      </div>
    </div>
  );
}

function Badge({ text }: { text: string }) {
  return (
    <span className="shrink-0 rounded bg-muted px-1 text-[10px] leading-4 text-muted-foreground">
      {text}
    </span>
  );
}

/** Compact select with a search box for long lists, arrow-key navigation and optional free text. */
export function Dropdown({
  label,
  value,
  options,
  onChange,
  allowCustom,
  placeholder,
}: DropdownProps) {
  const badge = options.find((o) => o.value === value)?.badge;
  return (
    <Popover
      className="min-w-0"
      panelClassName="left-0 right-0 top-full mt-1"
      trigger={({ open, toggle }) => (
        <button
          aria-expanded={open}
          aria-haspopup="listbox"
          aria-label={label}
          className="flex h-7 w-full min-w-0 items-center gap-1.5 rounded-md border bg-background px-2 text-left text-xs hover:bg-muted/50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
          onClick={toggle}
          title={value}
          type="button"
        >
          <span
            className={cn(
              'min-w-0 flex-1 truncate font-mono',
              value === undefined && 'font-sans text-muted-foreground',
            )}
          >
            {value ?? placeholder}
          </span>
          {badge ? <Badge text={badge} /> : null}
          <ChevronDown className="size-3 shrink-0 text-muted-foreground" />
        </button>
      )}
    >
      {(close) => (
        <Panel
          allowCustom={allowCustom}
          label={label}
          onChange={(v) => {
            onChange(v);
            close();
          }}
          options={options}
          value={value}
        />
      )}
    </Popover>
  );
}
