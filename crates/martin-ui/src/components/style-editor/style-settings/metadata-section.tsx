import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { setRoot } from '../operations';
import { MetadataJsonEditor } from './metadata-json-editor';
import type { StyleSectionProps } from './types';
import { SettingsSection } from './ui';

/** Free-form `metadata` and global `state` as validated JSON. */
export function MetadataSection({ style, apply }: StyleSectionProps) {
  return (
    <SettingsSection title="Metadata">
      <MetadataJsonEditor
        hint="Free-form data for tools; MapLibre ignores it."
        label="metadata"
        onCommit={(next) => apply('Edit metadata', (d) => setRoot(d, 'metadata', next))}
        value={style.metadata as Record<string, unknown> | undefined}
      />
      <MetadataJsonEditor
        hint="Default values for global-state expressions, one object per key."
        label="state"
        onCommit={(next) =>
          apply('Edit state', (d) => setRoot(d, 'state', next as StyleSpecification['state']))
        }
        value={style.state as Record<string, unknown> | undefined}
      />
    </SettingsSection>
  );
}
