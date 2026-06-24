export interface UnderlayProvider {
  id: string;
  label: string;
  tiles: string;
  attribution: string;
}

const OSM_ATTRIBUTION =
  '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>';
const CARTO_ATTRIBUTION = `${OSM_ATTRIBUTION} &copy; <a href="https://carto.com/attributions">CARTO</a>`;

export const UNDERLAY_PROVIDERS = [
  {
    attribution: OSM_ATTRIBUTION,
    id: 'osm',
    label: 'OSM',
    tiles: 'https://tile.openstreetmap.org/{z}/{x}/{y}.png',
  },
  {
    attribution: CARTO_ATTRIBUTION,
    id: 'positron',
    label: 'Positron',
    tiles: 'https://basemaps.cartocdn.com/light_all/{z}/{x}/{y}.png',
  },
  {
    attribution: CARTO_ATTRIBUTION,
    id: 'dark',
    label: 'Dark Matter',
    tiles: 'https://basemaps.cartocdn.com/dark_all/{z}/{x}/{y}.png',
  },
  {
    attribution: CARTO_ATTRIBUTION,
    id: 'voyager',
    label: 'Voyager',
    tiles: 'https://basemaps.cartocdn.com/rastertiles/voyager/{z}/{x}/{y}.png',
  },
] as const satisfies readonly UnderlayProvider[];

export type UnderlayProviderId = (typeof UNDERLAY_PROVIDERS)[number]['id'];

export const UNDERLAY_PROVIDER_IDS: readonly UnderlayProviderId[] = UNDERLAY_PROVIDERS.map(
  (p) => p.id,
);

export function findProvider(id: UnderlayProviderId | undefined): UnderlayProvider | undefined {
  return UNDERLAY_PROVIDERS.find((p) => p.id === id);
}
