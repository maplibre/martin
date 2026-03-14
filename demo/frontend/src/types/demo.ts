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
}
