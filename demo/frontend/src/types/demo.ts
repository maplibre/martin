/**
 * Demo layer shape for map and panels.
 * Mirrors the demo-layers content schema so React components don't depend on Astro content config.
 */
export interface AllowedParameter {
  name: string;
  type: 'number' | 'string' | 'range';
  label?: string;
  default?: string | number;
  min?: number;
  max?: number;
  options?: string[];
}

export interface DemoLayerEntry {
  id: string;
  label: string;
  layerType: 'fill' | 'line';
  sourceLayer: string;
  url: string;
  sqlTemplate: string;
  paint: Record<string, unknown>;
  allowedParameters?: AllowedParameter[];
  /** Feature property to use as the tile source promoteId. Defaults to "id". */
  promoteIdField?: string;
  /** If set, the map locks to these bounds when this layer is active. */
  viewBounds?: [[number, number], [number, number]];
  /** If set, the map flies to this center when this layer is active. */
  viewCenter?: [number, number];
  /** If set, the map flies to this zoom level when this layer is active. */
  viewZoom?: number;
  /** Feature property used as the hover tooltip display name. Defaults to "name". */
  hoverNameField?: string;
}

/**
 * Feature currently under the cursor on the map.
 * `name` is a resolved display label; `properties` carries all raw feature props.
 */
export interface HoveredFeature {
  name: string;
  properties: Record<string, unknown>;
}
