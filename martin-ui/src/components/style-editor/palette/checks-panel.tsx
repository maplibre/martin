import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { cn } from '@/lib/utils';
import { type Color, formatColor } from '../colors';
import { validateStyle } from '../style-spec';
import { type ContrastFix, contrastFixes, type LabelIssue, labelIssues } from './model';

/** Props of the checks tab. */
export interface ChecksPanelProps {
  /** Style being edited. */
  style: StyleSpecification;
  /** Applies one of the offered contrast fixes as a single undoable edit. */
  onFix: (issue: LabelIssue, fix: ContrastFix) => void;
  /** Opens a layer in the inspector. */
  onSelectLayer: (id: string) => void;
}

/** Label contrast and v8 validation, recomputed on every edit. */
export function ChecksPanel({ style, onFix, onSelectLayer }: ChecksPanelProps) {
  const issues = labelIssues(style);
  const errors = validateStyle(style);

  return (
    <div className="flex flex-col divide-y">
      <p className="px-3 py-3 text-[11px] text-muted-foreground">
        Rechecked after every edit. Each fix is one step you can undo with Ctrl+Z.
      </p>
      <section className="space-y-2 px-3 py-3">
        <Heading count={issues.length} title="Label contrast" />
        {issues.length === 0 ? (
          <Status text="Every literal label color reads at 3:1 or better" />
        ) : null}
        <ul className="-mx-3 divide-y divide-border/60">
          {issues.map((issue) => (
            <li className="space-y-2 px-3 py-2.5" key={issue.layer}>
              <button
                className="flex w-full min-w-0 items-center gap-2.5 rounded text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
                onClick={() => onSelectLayer(issue.layer)}
                title={`Open ${issue.layer}`}
                type="button"
              >
                <Sample against={issue.against} text={issue.text} />
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-mono text-xs">{issue.layer}</span>
                  <span className="block truncate text-[11px] text-muted-foreground tabular-nums">
                    {issue.ratio.toFixed(2)}:1 against its {issue.againstLabel}, aim for 4.5:1
                  </span>
                </span>
              </button>
              <div className="grid grid-cols-2 gap-1.5">
                {contrastFixes(style, issue).map((fix) => (
                  <button
                    className="flex h-7 min-w-0 items-center gap-1.5 rounded-md border bg-background px-1.5 text-xs hover:bg-muted focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
                    key={fix.label}
                    onClick={() => onFix(issue, fix)}
                    title={`Sets ${fix.changes.map((c) => `${c.name} to ${c.value}`).join(' and ')}. Undo with Ctrl+Z.`}
                    type="button"
                  >
                    <Sample against={fix.against} small text={fix.text} />
                    <span className="min-w-0 truncate">{fix.label}</span>
                    <span className="ml-auto shrink-0 font-mono text-[11px] text-muted-foreground tabular-nums">
                      {fix.ratio.toFixed(1)}:1
                    </span>
                  </button>
                ))}
              </div>
            </li>
          ))}
        </ul>
      </section>
      <section className="space-y-2 px-3 py-3">
        <Heading count={errors.length} title="Spec validation" />
        {errors.length === 0 ? <Status text="Valid MapLibre style (v8)" /> : null}
        <ul className="space-y-1.5">
          {errors.map((message) => (
            <li className="flex gap-2 text-[11px]" key={message}>
              <span className="mt-1.5 size-1.5 shrink-0 rounded-full bg-destructive" />
              <span className="min-w-0 break-words font-mono text-foreground">{message}</span>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}

function Heading({ title, count }: { title: string; count: number }) {
  return (
    <h3 className="flex items-center justify-between text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
      {title}
      {count > 0 ? <span className="font-mono tabular-nums">{count}</span> : null}
    </h3>
  );
}

function Status({ text }: { text: string }) {
  return (
    <p className="flex items-center gap-2 text-xs text-muted-foreground">
      <span className="size-1.5 shrink-0 rounded-full bg-success" />
      {text}
    </p>
  );
}

function Sample({ text, against, small }: { text: Color; against: Color; small?: boolean }) {
  return (
    <span
      className={cn(
        'flex shrink-0 items-center justify-center rounded font-semibold ring-1 ring-inset ring-foreground/10',
        small ? 'h-5 w-7 text-[11px]' : 'h-7 w-10 text-sm',
      )}
      style={{ background: formatColor(against), color: formatColor(text) }}
    >
      Aa
    </span>
  );
}
