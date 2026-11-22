import { TriangleAlert } from 'lucide-react';
import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode } from 'react';
import { useState } from 'react';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

const FOCUS = 'focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent';

/** Props of {@link SettingsSection}. */
export interface SettingsSectionProps {
  /** Section heading, the only uppercase text of the tab. */
  title: string;
  /** Optional controls at the end of the heading row. */
  actions?: ReactNode;
  /** Section body, usually groups. */
  children: ReactNode;
}

/** A titled section of the Style tab. */
export function SettingsSection({ title, actions, children }: SettingsSectionProps) {
  return (
    <section aria-label={title} className="space-y-3 px-3 py-3">
      <div className="flex min-h-6 items-center justify-between gap-2">
        <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
          {title}
        </h3>
        {actions}
      </div>
      {children}
    </section>
  );
}

/** Props of {@link SettingsGroup}. */
export interface SettingsGroupProps {
  /** Group title in normal case. */
  title: string;
  /** Optional controls at the end of the title row. */
  actions?: ReactNode;
  /** Group body: hints, rows and lists. */
  children: ReactNode;
}

/** A titled sub-block of a section. */
export function SettingsGroup({ title, actions, children }: SettingsGroupProps) {
  return (
    <div className="space-y-2">
      <div className="flex min-h-7 items-center justify-between gap-2">
        <h4 className="min-w-0 truncate text-xs font-medium text-foreground">{title}</h4>
        {actions}
      </div>
      {children}
    </div>
  );
}

/** Props of {@link FieldRow}. */
export interface FieldRowProps {
  /** Text of the label column. */
  label: string;
  /** Plain-language explanation shown in a tooltip on the label. */
  hint?: string;
  /** Trailing control such as a reset button; an empty slot keeps the columns aligned. */
  action?: ReactNode;
  /** Align the label to the top of a tall control. */
  alignTop?: boolean;
  /** The control. */
  children: ReactNode;
}

/** One label, control and trailing action in the Layer tab's `112px / control / action` grid. */
export function FieldRow({ label, hint, action, alignTop, children }: FieldRowProps) {
  const text = (
    <span
      className={cn('min-w-0 truncate text-xs text-foreground', alignTop && 'pt-4')}
      title={label}
    >
      {label}
    </span>
  );
  return (
    <div
      className={cn(
        'grid grid-cols-[112px_minmax(0,1fr)_auto] gap-2',
        alignTop ? 'items-start' : 'items-center',
      )}
    >
      {hint ? (
        <Tooltip>
          <TooltipTrigger asChild>{text}</TooltipTrigger>
          <TooltipContent className="max-w-72 text-xs" side="left">
            {hint}
          </TooltipContent>
        </Tooltip>
      ) : (
        text
      )}
      <div className="min-w-0">{children}</div>
      {action ?? <span className="size-6" />}
    </div>
  );
}

/** A muted one-sentence explanation, placed under the thing it explains. */
export function Hint({ children }: { children: ReactNode }) {
  return <p className="text-[11px] text-muted-foreground">{children}</p>;
}

/** Status shown by a {@link StatusLine} dot. */
export type StatusTone = 'ok' | 'warning' | 'error' | 'pending';

const DOT: Record<StatusTone, string> = {
  error: 'bg-destructive',
  ok: 'bg-success',
  pending: 'bg-muted-foreground/40',
  warning: 'bg-amber-500',
};

/** A 6px status dot followed by muted text; only errors tint the text. */
export function StatusLine({ tone, children }: { tone: StatusTone; children: string }) {
  return (
    <p
      className={cn(
        'flex min-w-0 items-center gap-1.5 text-[11px]',
        tone === 'error' ? 'text-destructive' : 'text-muted-foreground',
      )}
      role="status"
    >
      <span aria-hidden className={cn('size-1.5 shrink-0 rounded-full', DOT[tone])} />
      <span className="min-w-0 truncate" title={children}>
        {children}
      </span>
    </p>
  );
}

/** A neutral bordered box with an amber warning icon, for warnings that need explaining. */
export function WarningBox({ children }: { children: ReactNode }) {
  return (
    <div className="flex gap-2 rounded-md border bg-muted/30 p-2">
      <TriangleAlert className="mt-0.5 size-3.5 shrink-0 text-amber-500" />
      <div className="min-w-0 flex-1 space-y-1.5 break-words text-[11px] text-muted-foreground">
        {children}
      </div>
    </div>
  );
}

/** A monospace chip for one value such as a font name or a placeholder. */
export function Chip({ children }: { children: string }) {
  return (
    <span
      className="max-w-full truncate rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] text-foreground"
      title={children}
    >
      {children}
    </span>
  );
}

/** A wrapping row of {@link Chip}s. */
export function ChipList({ items }: { items: string[] }) {
  return (
    <div className="flex flex-wrap gap-1">
      {items.map((item) => (
        <Chip key={item}>{item}</Chip>
      ))}
    </div>
  );
}

/** A small neutral badge, such as a source type. */
export function Badge({ children }: { children: string }) {
  return (
    <span className="shrink-0 rounded bg-muted px-1 text-[10px] leading-4 text-muted-foreground">
      {children}
    </span>
  );
}

/** A small bordered button for secondary actions. */
export function SecondaryButton({ className, ...props }: ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      className={cn(
        'inline-flex h-7 shrink-0 items-center justify-center gap-1.5 rounded-md border bg-background px-2 text-xs hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50',
        FOCUS,
        className,
      )}
      type="button"
      {...props}
    />
  );
}

/** The accent button of a section, for its one primary action. */
export function PrimaryButton({ className, ...props }: ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      className={cn(
        'inline-flex h-7 shrink-0 items-center justify-center gap-1.5 rounded-md bg-editor-accent px-2.5 text-xs text-white hover:bg-editor-accent/90 disabled:cursor-not-allowed disabled:opacity-50',
        FOCUS,
        className,
      )}
      type="button"
      {...props}
    />
  );
}

/** Props of {@link IconButton}. */
export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  /** Accessible name, also shown as the tooltip. */
  label: string;
}

/** A 24px icon-only button with a tooltip. */
export function IconButton({ label, className, children, ...props }: IconButtonProps) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          aria-label={label}
          className={cn(
            'flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground',
            FOCUS,
            className,
          )}
          type="button"
          {...props}
        >
          {children}
        </button>
      </TooltipTrigger>
      <TooltipContent className="text-xs">{label}</TooltipContent>
    </Tooltip>
  );
}

const INPUT =
  'h-8 w-full min-w-0 rounded-md border bg-background px-2 text-xs placeholder:text-muted-foreground/60';

/** A text input; `mono` is for URLs, ids and other values. */
export function TextInput({
  mono,
  className,
  ...props
}: InputHTMLAttributes<HTMLInputElement> & { mono?: boolean }) {
  return (
    <input
      className={cn(INPUT, FOCUS, mono && 'font-mono text-[11px]', className)}
      type="text"
      {...props}
    />
  );
}

/** Props of {@link NumberInput}. */
export interface NumberInputProps {
  /** Accessible name. */
  label: string;
  /** Current number, `undefined` shows an empty field. */
  value: number | undefined;
  /** Shown muted while the field is empty. */
  placeholder?: number;
  /** Lower bound; typed values are clamped to it. */
  min?: number;
  /** Upper bound; typed values are clamped to it. */
  max?: number;
  /** Step of the arrow keys. */
  step?: number;
  /** Unit shown muted inside the field, such as `°`. */
  suffix?: string;
  /** Called with each valid number, or `undefined` when the field is emptied. */
  onChange: (next: number | undefined) => void;
}

/** A compact number field without spinners, with an optional muted unit. */
export function NumberInput({
  label,
  value,
  placeholder,
  min,
  max,
  step,
  suffix,
  onChange,
}: NumberInputProps) {
  const [draft, setDraft] = useState<string | null>(null);
  return (
    <div className="relative min-w-0">
      <input
        aria-label={label}
        className={cn(
          INPUT,
          FOCUS,
          'tabular-nums [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none',
          suffix && 'pr-5',
        )}
        max={max}
        min={min}
        onBlur={() => setDraft(null)}
        onChange={(e) => {
          setDraft(e.target.value);
          if (e.target.value.trim() === '') {
            onChange(undefined);
            return;
          }
          const n = Number.parseFloat(e.target.value);
          if (Number.isFinite(n)) {
            onChange(Math.min(max ?? Number.POSITIVE_INFINITY, Math.max(min ?? -Infinity, n)));
          }
        }}
        placeholder={placeholder === undefined ? undefined : String(placeholder)}
        step={step}
        type="number"
        value={draft ?? value ?? ''}
      />
      {suffix ? (
        <span className="pointer-events-none absolute inset-y-0 right-2 flex items-center text-[11px] text-muted-foreground">
          {suffix}
        </span>
      ) : null}
    </div>
  );
}
