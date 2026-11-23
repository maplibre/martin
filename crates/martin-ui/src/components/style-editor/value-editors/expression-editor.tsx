import { useLayoutEffect, useRef, useState } from 'react';
import { cn } from '@/lib/utils';
import { type PropertySpec, validateProperty } from '../style-spec';

/** Props of the raw expression editor. */
export interface ExpressionEditorProps {
  /** Property name, used in validation messages. */
  name: string;
  /** Definition of the property. */
  spec: PropertySpec;
  /** The expression. */
  value: unknown;
  /** Called with a parsed, spec-valid expression only. */
  onChange: (next: unknown) => void;
}

const MAX_HEIGHT_PX = 16 * 16;

/** JSON with short arrays kept on one line, so expressions read like code instead of one token per line. */
function pretty(value: unknown, indent = ''): string {
  const flat = JSON.stringify(value) ?? 'null';
  if (!Array.isArray(value) || flat.length + indent.length <= 60) return flat;
  const inner = `${indent}  `;
  const [head, ...rest] = value;
  return `[${JSON.stringify(head)},\n${rest.map((v) => inner + pretty(v, inner)).join(',\n')}\n${indent}]`;
}

function check(
  text: string,
  name: string,
  spec: PropertySpec,
): { parsed?: unknown; errors: string[] } {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch (e) {
    return { errors: [e instanceof Error ? e.message : 'Invalid JSON'] };
  }
  const errors = validateProperty(parsed, name, spec);
  return errors.length > 0 ? { errors } : { errors, parsed };
}

/** JSON text area validated against the v8 spec on every keystroke. */
export function ExpressionEditor({ name, spec, value, onChange }: ExpressionEditorProps) {
  const canonical = pretty(value);
  const [text, setText] = useState(canonical);
  const [seen, setSeen] = useState(canonical);
  const ref = useRef<HTMLTextAreaElement>(null);

  if (canonical !== seen) {
    setSeen(canonical);
    setText(canonical);
  }

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, MAX_HEIGHT_PX)}px`;
  }, [text]);

  const { parsed, errors } = check(text, name, spec);
  const valid = errors.length === 0;

  const commit = () => {
    if (valid && text !== canonical) onChange(parsed);
  };

  return (
    <div className="flex min-w-0 flex-col gap-1">
      <textarea
        aria-invalid={!valid}
        aria-label={`${name} expression`}
        className={cn(
          'block w-full resize-none overflow-y-auto whitespace-pre-wrap break-all rounded-md border bg-background px-2 py-1.5 font-mono text-[11px] leading-4',
          'focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent',
          !valid && 'border-destructive',
        )}
        onBlur={commit}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            commit();
          } else if (e.key === 'Escape') {
            e.preventDefault();
            setText(canonical);
          }
        }}
        ref={ref}
        rows={1}
        spellCheck={false}
        value={text}
      />
      {errors.map((message) => (
        <div className="break-words text-[11px] text-destructive" key={message}>
          {message}
        </div>
      ))}
      <div className="flex items-center justify-between text-[11px] text-muted-foreground">
        <span>Ctrl+Enter or blur applies, Esc reverts</span>
        <button
          className="rounded-md border px-1.5 py-0.5 hover:bg-muted disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
          disabled={!valid}
          onClick={() => setText(pretty(parsed))}
          type="button"
        >
          Format
        </button>
      </div>
    </div>
  );
}
