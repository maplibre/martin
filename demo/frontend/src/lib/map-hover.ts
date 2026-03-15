import type { DemoLayerEntry, HoveredFeature } from '@/types/demo';

/**
 * Build a HoveredFeature from a raw GeoJSON feature returned by MapLibre.
 * The display name is derived from the layer's `hoverNameField` (if set) or
 * falls back to the standard "name" property.
 */
export function buildHoveredFeature(
  src: DemoLayerEntry,
  properties: Record<string, unknown>,
): HoveredFeature {
  const nameField = src.hoverNameField ?? 'name';
  const raw = properties[nameField];
  const name =
    raw !== undefined && raw !== null
      ? nameField === src.hoverNameField && src.hoverNameField !== 'name'
        ? `Zone ${raw}`
        : String(raw)
      : '';

  return { name, properties };
}
