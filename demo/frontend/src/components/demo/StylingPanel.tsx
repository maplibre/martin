interface StylingPanelProps {
  spriteType: 'sdf' | 'plain';
  styleId: string;
  fontStack: string;
  onSpriteTypeChange: (value: 'sdf' | 'plain') => void;
  onStyleChange: (value: string) => void;
  onFontChange: (value: string) => void;
}

const STYLE_OPTIONS = [
  { id: 'dark', label: 'Toner' },
  { id: 'light', label: 'Positron' },
] as const;

const FONT_OPTIONS = [
  { id: 'default', label: 'Default' },
  { id: 'noto', label: 'Noto Sans' },
] as const;

export default function StylingPanel({
  spriteType,
  styleId,
  fontStack,
  onSpriteTypeChange,
  onStyleChange,
  onFontChange,
}: StylingPanelProps) {
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-1">
        <span className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider">
          Sprites
        </span>
        <div className="flex gap-1">
          <button
            className={`px-2 py-1 text-[10px] font-mono rounded transition-colors ${
              spriteType === 'sdf'
                ? 'bg-primary text-primary-foreground'
                : 'text-muted-foreground hover:text-foreground border border-border'
            }`}
            onClick={() => onSpriteTypeChange('sdf')}
            type="button"
          >
            SDF
          </button>
          <button
            className={`px-2 py-1 text-[10px] font-mono rounded transition-colors ${
              spriteType === 'plain'
                ? 'bg-primary text-primary-foreground'
                : 'text-muted-foreground hover:text-foreground border border-border'
            }`}
            onClick={() => onSpriteTypeChange('plain')}
            type="button"
          >
            Plain
          </button>
        </div>
      </div>
      <div className="flex flex-col gap-1">
        <span className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider">
          Style
        </span>
        <select
          aria-label="Map style"
          className="rounded border border-border bg-background px-2 py-1 text-[11px] font-mono text-foreground w-full max-w-[140px]"
          onChange={(e) => onStyleChange(e.target.value)}
          value={styleId}
        >
          {STYLE_OPTIONS.map((opt) => (
            <option key={opt.id} value={opt.id}>
              {opt.label}
            </option>
          ))}
        </select>
      </div>
      <div className="flex flex-col gap-1">
        <span className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider">
          Fonts
        </span>
        <select
          aria-label="Font stack"
          className="rounded border border-border bg-background px-2 py-1 text-[11px] font-mono text-foreground w-full max-w-[140px]"
          onChange={(e) => onFontChange(e.target.value)}
          value={fontStack}
        >
          {FONT_OPTIONS.map((opt) => (
            <option key={opt.id} value={opt.id}>
              {opt.label}
            </option>
          ))}
        </select>
      </div>
    </div>
  );
}
