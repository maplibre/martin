import { type ReactNode, useEffect, useId, useRef } from 'react';

interface BottomSheetProps {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
}

export default function BottomSheet({ open, onClose, title, children }: BottomSheetProps) {
  const titleId = useId();
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const prevOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => {
      document.body.style.overflow = prevOverflow;
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [open, onClose]);

  useEffect(() => {
    if (open && panelRef.current) {
      panelRef.current.focus();
    }
  }, [open]);

  if (!open) return null;

  return (
    <div
      aria-labelledby={titleId}
      aria-modal="true"
      className="fixed inset-0 z-40 flex flex-col justify-end"
      role="dialog"
    >
      <button
        aria-label="Close"
        className="absolute inset-0 bg-black/50 backdrop-blur-sm"
        onClick={onClose}
        type="button"
      />
      <div
        className="relative z-10 bg-background border border-b-0 border-border rounded-t-xl shadow-xl max-h-[85vh] flex flex-col overflow-hidden focus:outline-none"
        ref={panelRef}
        tabIndex={-1}
      >
        <div className="flex items-center justify-between px-4 py-3 border-b border-border shrink-0">
          <span className="text-sm font-mono font-medium" id={titleId}>
            {title}
          </span>
          <button
            aria-label="Close"
            className="text-muted-foreground hover:text-foreground transition-colors p-1 rounded font-mono text-sm"
            onClick={onClose}
            type="button"
          >
            Close
          </button>
        </div>
        <div className="overflow-y-auto overscroll-contain min-h-0 flex-1">{children}</div>
      </div>
    </div>
  );
}
