import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import type { Map as MapLibre } from 'maplibre-gl';
import type { StyleDocument } from '../use-style-document';

/** Props every section of the Style tab receives. */
export interface StyleSectionProps {
  /** The style being edited. */
  style: StyleSpecification;
  /** Applies one recorded, undoable edit; `coalesce` merges rapid edits into one history entry. */
  apply: StyleDocument['apply'];
  /** The live map, for reading the current camera; null until it loads. */
  map: MapLibre | null;
}
