import { ChevronRight } from 'lucide-react';
import { type ReactNode, useState } from 'react';
import { cn } from '@/lib/utils';
import { Hint, SecondaryButton } from './ui';

/** Props of {@link RenderingGroup}. */
export interface RenderingGroupProps {
  /** Group title in normal case. */
  title: string;
  /** One sentence on what the group does. */
  hint: string;
  /** Muted state shown next to the title, such as `Not set` or `2 values`. */
  state: string;
  /** Whether the style has this group. */
  isSet: boolean;
  /** Creates the group; omit the handler to disable the button. */
  onAdd?: () => void;
  /** Text under a disabled Add button. */
  addBlocked?: string;
  /** Removes the group from the style. */
  onRemove: () => void;
  /** Fields of the group. */
  children: ReactNode;
}

/** A collapsible block that is open while the style has it and offers Add or Remove. */
export function RenderingGroup({
  title,
  hint,
  state,
  isSet,
  onAdd,
  addBlocked,
  onRemove,
  children,
}: RenderingGroupProps) {
  const [forced, setForced] = useState<boolean | null>(null);
  const open = forced ?? isSet;
  return (
    <fieldset aria-label={title} className="m-0 min-w-0 rounded-md border p-0">
      <div className="flex h-9 items-center gap-2 px-2">
        <button
          aria-expanded={open}
          aria-label={`${title} group`}
          className="flex min-w-0 flex-1 items-center gap-1.5 rounded-sm text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
          onClick={() => setForced(!open)}
          type="button"
        >
          <ChevronRight
            className={cn(
              'size-3.5 shrink-0 text-muted-foreground transition-transform',
              open && 'rotate-90',
            )}
          />
          <span className="text-xs font-medium text-foreground">{title}</span>
          <span className="min-w-0 truncate text-[11px] text-muted-foreground">{state}</span>
        </button>
        {isSet ? (
          <SecondaryButton onClick={onRemove}>Remove</SecondaryButton>
        ) : (
          <SecondaryButton disabled={!onAdd} onClick={onAdd}>
            Add
          </SecondaryButton>
        )}
      </div>
      {open ? (
        <div className="space-y-2 border-t px-2 py-2">
          <Hint>{hint}</Hint>
          {!isSet && addBlocked ? <Hint>{addBlocked}</Hint> : null}
          {children}
        </div>
      ) : null}
    </fieldset>
  );
}
