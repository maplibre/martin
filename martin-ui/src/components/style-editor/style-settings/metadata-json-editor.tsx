import { useState } from 'react';
import { cn } from '@/lib/utils';
import { Hint, SecondaryButton, SettingsGroup } from './ui';

/** Props of {@link MetadataJsonEditor}. */
export interface MetadataJsonEditorProps {
  /** Accessible name and heading of the editor. */
  label: string;
  /** Current object in the style, `undefined` when unset. */
  value: Record<string, unknown> | undefined;
  /** Called with a valid object, or `undefined` when the text is emptied. */
  onCommit: (next: Record<string, unknown> | undefined) => void;
  /** One muted line under the editor. */
  hint: string;
}

type Parsed =
  | { ok: true; value: Record<string, unknown> | undefined }
  | { ok: false; error: string };

function parse(text: string): Parsed {
  if (text.trim() === '') return { ok: true, value: undefined };
  try {
    const json: unknown = JSON.parse(text);
    if (typeof json !== 'object' || json === null || Array.isArray(json))
      return { error: 'Must be a JSON object', ok: false };
    return { ok: true, value: json as Record<string, unknown> };
  } catch (e) {
    return { error: e instanceof Error ? e.message : 'Invalid JSON', ok: false };
  }
}

function format(value: Record<string, unknown> | undefined): string {
  return value === undefined ? '' : JSON.stringify(value, null, 2);
}

/** A monospace JSON object editor that validates while typing and commits on blur or Ctrl/Cmd+Enter. */
export function MetadataJsonEditor({ label, value, onCommit, hint }: MetadataJsonEditorProps) {
  const [draft, setDraft] = useState<string | null>(null);
  const text = draft ?? format(value);
  const parsed = parse(text);

  const commit = () => {
    if (draft === null || !parsed.ok) return;
    if (format(parsed.value) !== format(value)) onCommit(parsed.value);
    setDraft(null);
  };

  const rows = Math.min(10, Math.max(3, text.split('\n').length));

  return (
    <SettingsGroup
      actions={
        <SecondaryButton
          aria-label={`Format ${label}`}
          disabled={!parsed.ok || text === ''}
          onClick={() => parsed.ok && setDraft(format(parsed.value))}
        >
          Format
        </SecondaryButton>
      }
      title={label}
    >
      <textarea
        aria-invalid={!parsed.ok}
        aria-label={label}
        className={cn(
          'w-full resize-none rounded-md border bg-background p-2 font-mono text-[11px] leading-snug placeholder:text-muted-foreground/60 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent',
          !parsed.ok && 'border-destructive',
        )}
        onBlur={commit}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            commit();
          }
        }}
        placeholder="{}"
        rows={rows}
        spellCheck={false}
        value={text}
      />
      {parsed.ok ? (
        <Hint>{hint}</Hint>
      ) : (
        <p className="break-words text-[11px] text-destructive" role="alert">
          {parsed.error}
        </p>
      )}
    </SettingsGroup>
  );
}
