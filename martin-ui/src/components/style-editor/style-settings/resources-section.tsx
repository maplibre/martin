import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { Pencil, Plus, X } from 'lucide-react';
import { useState } from 'react';
import { setRoot } from '../operations';
import { martinGlyphsUrl, martinSpriteUrl, useStyleResources } from '../style-resources';
import {
  fetchChecked,
  fetchJson,
  glyphUrl,
  missingGlyphTokens,
  type ProbeState,
  type SpriteSheet,
  spriteSheets,
  spriteValue,
  useProbe,
} from './resources-probe';
import type { StyleSectionProps } from './types';
import {
  Chip,
  ChipList,
  FieldRow,
  Hint,
  IconButton,
  PrimaryButton,
  SecondaryButton,
  SettingsGroup,
  SettingsSection,
  StatusLine,
  TextInput,
  WarningBox,
} from './ui';

/** Where fonts (`glyphs`) and icons (`sprite`) come from, with one-click Martin options. */
export function ResourcesSection(props: StyleSectionProps) {
  return (
    <SettingsSection title="Fonts and icons">
      <FontsBlock {...props} />
      <IconsBlock {...props} />
    </SettingsSection>
  );
}

function ProbeLine({ state }: { state: ProbeState }) {
  if (state.status === 'idle') return null;
  if (state.status === 'loading') return <StatusLine tone="pending">Checking</StatusLine>;
  return <StatusLine tone={state.status}>{state.text}</StatusLine>;
}

function CommitInput({
  label,
  value,
  onCommit,
  placeholder,
  invalid,
}: {
  label: string;
  value: string;
  onCommit: (next: string) => void;
  placeholder?: string;
  invalid?: boolean;
}) {
  const [draft, setDraft] = useState<string>();
  const commit = () => {
    if (draft !== undefined && draft !== value) onCommit(draft);
    setDraft(undefined);
  };
  return (
    <TextInput
      aria-invalid={invalid}
      aria-label={label}
      className={invalid ? 'border-amber-500' : undefined}
      mono
      onBlur={commit}
      onChange={(e) => setDraft(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') e.currentTarget.blur();
        else if (e.key === 'Escape') {
          setDraft(undefined);
          e.currentTarget.blur();
        }
      }}
      placeholder={placeholder}
      spellCheck={false}
      title={value}
      value={draft ?? value}
    />
  );
}

function FontsBlock({ style, apply }: StyleSectionProps) {
  const { fontsInStyle, martinFonts } = useStyleResources();
  const glyphs = style.glyphs ?? '';
  const missingTokens = glyphs === '' ? [] : missingGlyphTokens(glyphs);
  const probeFont = fontsInStyle[0];
  const probeable = glyphs !== '' && missingTokens.length === 0 && probeFont !== undefined;
  const probe = useProbe(
    probeable ? `glyphs ${glyphUrl(glyphs, probeFont, '0-255')}` : undefined,
    async (signal) => {
      await fetchChecked(glyphUrl(glyphs, probeFont, '0-255'), signal);
      return `Loads ${probeFont}`;
    },
  );
  const martinUrl = martinGlyphsUrl();
  const usingMartin = glyphs === martinUrl;
  const unserved = fontsInStyle.filter((f) => !martinFonts.includes(f));

  const setGlyphs = (next: string, label: string) =>
    apply(label, (draft) => setRoot(draft, 'glyphs', next === '' ? undefined : next));

  return (
    <SettingsGroup title="Fonts (glyphs)">
      <CommitInput
        invalid={missingTokens.length > 0}
        label="Glyphs URL"
        onCommit={(next) => setGlyphs(next, next === '' ? 'Remove glyphs URL' : 'Set glyphs URL')}
        placeholder="https://example.com/fonts/{fontstack}/{range}.pbf"
        value={glyphs}
      />
      <Hint>Every font name a text layer uses must exist on this server.</Hint>
      {missingTokens.length > 0 ? (
        <WarningBox>
          <p>The URL is missing</p>
          <ChipList items={missingTokens} />
        </WarningBox>
      ) : null}
      {probeable ? (
        <ProbeLine state={probe} />
      ) : glyphs === '' ? (
        <StatusLine tone="warning">No glyphs URL: text layers fall back to local fonts</StatusLine>
      ) : null}
      <div>
        <SecondaryButton
          disabled={usingMartin}
          onClick={() => setGlyphs(martinUrl, 'Use Martin fonts')}
        >
          {usingMartin ? 'Using Martin fonts' : 'Use Martin fonts'}
        </SecondaryButton>
      </div>
      {martinFonts.length > 0 ? (
        <div className="space-y-1">
          <Hint>Martin serves these fonts.</Hint>
          <ChipList items={martinFonts} />
        </div>
      ) : null}
      {martinFonts.length > 0 && unserved.length > 0 ? (
        <WarningBox>
          <p>
            {usingMartin
              ? 'Martin does not serve these fonts.'
              : 'Martin does not serve these fonts, so they would stop rendering.'}
          </p>
          <ChipList items={unserved} />
        </WarningBox>
      ) : null}
    </SettingsGroup>
  );
}

function SheetRow({
  sheet,
  onEdit,
  onRemove,
}: {
  sheet: SpriteSheet;
  onEdit: (url: string) => void;
  onRemove: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const probe = useProbe(`sprite ${sheet.url}`, async (signal) => {
    const index = await fetchJson(`${sheet.url}.json`, signal);
    const count = index && typeof index === 'object' ? Object.keys(index).length : 0;
    return `${count} icons`;
  });
  return (
    <li className="space-y-1 rounded-md border px-2 py-1.5">
      <div className="flex items-center gap-1.5">
        <Chip>{sheet.id}</Chip>
        {editing ? (
          <div className="min-w-0 flex-1">
            <CommitInput
              label={`URL of sheet ${sheet.id}`}
              onCommit={(next) => {
                if (next.trim() !== '') onEdit(next.trim());
                setEditing(false);
              }}
              value={sheet.url}
            />
          </div>
        ) : (
          <span
            className="min-w-0 flex-1 truncate font-mono text-[11px] text-muted-foreground"
            title={sheet.url}
          >
            {sheet.url}
          </span>
        )}
        <IconButton label={`Edit URL of sheet ${sheet.id}`} onClick={() => setEditing(!editing)}>
          <Pencil className="size-3" />
        </IconButton>
        <IconButton label={`Remove sheet ${sheet.id}`} onClick={onRemove}>
          <X className="size-3.5" />
        </IconButton>
      </div>
      <ProbeLine state={probe} />
    </li>
  );
}

function AddSheet({
  sheets,
  onAdd,
}: {
  sheets: SpriteSheet[];
  onAdd: (sheet: SpriteSheet) => void;
}) {
  const [open, setOpen] = useState(false);
  const [id, setId] = useState('');
  const [url, setUrl] = useState('');
  if (!open) {
    return (
      <div>
        <SecondaryButton onClick={() => setOpen(true)}>
          <Plus className="size-3" /> Add sheet
        </SecondaryButton>
      </div>
    );
  }
  const taken = sheets.some((s) => s.id === id.trim());
  const valid = id.trim() !== '' && !taken && url.trim() !== '';
  return (
    <div className="space-y-2 rounded-md border p-2">
      <FieldRow label="Id">
        <TextInput
          aria-label="Sheet id"
          mono
          onChange={(e) => setId(e.target.value)}
          placeholder="default"
          value={id}
        />
      </FieldRow>
      {taken ? <StatusLine tone="error">Id is already used</StatusLine> : null}
      <FieldRow label="URL">
        <TextInput
          aria-label="Sheet URL"
          mono
          onChange={(e) => setUrl(e.target.value)}
          placeholder="https://example.com/sprite"
          value={url}
        />
      </FieldRow>
      <div className="flex gap-2">
        <PrimaryButton
          disabled={!valid}
          onClick={() => {
            onAdd({ id: id.trim(), url: url.trim() });
            setOpen(false);
            setId('');
            setUrl('');
          }}
        >
          Add
        </PrimaryButton>
        <SecondaryButton onClick={() => setOpen(false)}>Cancel</SecondaryButton>
      </div>
    </div>
  );
}

function IconsBlock({ style, apply }: StyleSectionProps) {
  const { martinSprites } = useStyleResources();
  const sheets = spriteSheets(style.sprite as string | SpriteSheet[] | undefined);

  const write = (next: SpriteSheet[], label: string) =>
    apply(label, (draft) =>
      setRoot(draft, 'sprite', spriteValue(next) as StyleSpecification['sprite']),
    );

  const quickAdd = (martinId: string, sdf: boolean) => {
    const url = martinSpriteUrl(martinId, sdf);
    const base = sheets.length === 0 && !sdf ? 'default' : sdf ? `${martinId}-sdf` : martinId;
    let id = base;
    for (let n = 2; sheets.some((s) => s.id === id); n++) id = `${base}-${n}`;
    write([...sheets, { id, url }], `Add ${sdf ? 'SDF ' : ''}sprite ${martinId}`);
  };

  return (
    <SettingsGroup title="Icons (sprite)">
      <Hint>Icons of non-default sheets are prefixed with the sheet id.</Hint>
      {sheets.length === 0 ? (
        <StatusLine tone="warning">No sprite: icon layers cannot draw</StatusLine>
      ) : null}
      <ul className="space-y-2">
        {sheets.map((sheet) => (
          <SheetRow
            key={sheet.id}
            onEdit={(url) =>
              write(
                sheets.map((s) => (s.id === sheet.id ? { ...s, url } : s)),
                `Set URL of sprite sheet ${sheet.id}`,
              )
            }
            onRemove={() =>
              write(
                sheets.filter((s) => s.id !== sheet.id),
                `Remove sprite sheet ${sheet.id}`,
              )
            }
            sheet={sheet}
          />
        ))}
      </ul>
      <AddSheet
        onAdd={(sheet) => write([...sheets, sheet], `Add sprite sheet ${sheet.id}`)}
        sheets={sheets}
      />
      {martinSprites.length > 0 ? (
        <div className="space-y-1.5">
          <Hint>Martin serves these sprites; SDF icons can be recolored.</Hint>
          {martinSprites.map((id) => (
            <div className="flex items-center gap-2" key={id}>
              <span className="min-w-0 flex-1 truncate font-mono text-xs" title={id}>
                {id}
              </span>
              <SecondaryButton
                aria-label={`Add sprite ${id}`}
                disabled={sheets.some((s) => s.url === martinSpriteUrl(id))}
                onClick={() => quickAdd(id, false)}
              >
                Add
              </SecondaryButton>
              <SecondaryButton
                aria-label={`Add SDF sprite ${id}`}
                disabled={sheets.some((s) => s.url === martinSpriteUrl(id, true))}
                onClick={() => quickAdd(id, true)}
              >
                Add SDF
              </SecondaryButton>
            </div>
          ))}
        </div>
      ) : null}
    </SettingsGroup>
  );
}
