import { Crosshair, Plane } from 'lucide-react';
import type { ReactNode } from 'react';
import { setRoot } from '../operations';
import { rootSpec, SpecRow } from './spec-row';
import type { StyleSectionProps } from './types';
import {
  Hint,
  NumberInput,
  PrimaryButton,
  SecondaryButton,
  SettingsGroup,
  SettingsSection,
} from './ui';

const NAME = rootSpec('$root', 'name');

function round(n: number, digits: number): number {
  const f = 10 ** digits;
  return Math.round(n * f) / f;
}

/** Style name and the camera the map opens with (center, zoom, bearing, pitch, roll). */
export function GeneralSection({ style, apply, map }: StyleSectionProps) {
  const [lng, lat] = style.center ?? [undefined, undefined];
  const cameraSet = (['center', 'zoom', 'bearing', 'pitch', 'roll'] as const).some(
    (k) => style[k] !== undefined,
  );

  const setNumber = (key: 'zoom' | 'bearing' | 'pitch' | 'roll', next: number | undefined) =>
    apply(
      next === undefined ? `Reset ${key}` : `Set ${key}`,
      (d) => setRoot(d, key, next),
      `root-${key}`,
    );

  const setCenter = (nextLng: number | undefined, nextLat: number | undefined) =>
    apply(
      'Set center',
      (d) => setRoot(d, 'center', [nextLng ?? lng ?? 0, nextLat ?? lat ?? 0]),
      'root-center',
    );

  const resetCamera = () =>
    apply('Reset camera', (d) => {
      for (const key of ['center', 'zoom', 'bearing', 'pitch', 'roll'] as const) {
        setRoot(d, key, undefined);
      }
    });

  const copyCurrentView = () => {
    if (!map) return;
    const c = map.getCenter();
    apply('Use current map view', (d) => {
      setRoot(d, 'center', [round(c.lng, 5), round(c.lat, 5)]);
      setRoot(d, 'zoom', round(map.getZoom(), 2));
      setRoot(d, 'bearing', round(map.getBearing(), 1));
      setRoot(d, 'pitch', round(map.getPitch(), 1));
    });
  };

  const flyToStyleView = () =>
    map?.jumpTo({
      bearing: style.bearing ?? 0,
      center: [lng ?? 0, lat ?? 0],
      pitch: style.pitch ?? 0,
      zoom: style.zoom ?? 0,
    });

  return (
    <SettingsSection title="General">
      <SpecRow
        hint="Shown in tools and the style list; MapLibre does not render it."
        label="Name"
        onChange={(next) => apply('Rename style', (d) => setRoot(d, 'name', next as string))}
        onReset={() => apply('Clear style name', (d) => setRoot(d, 'name', undefined))}
        spec={NAME}
        value={style.name}
      />
      <SettingsGroup
        actions={
          cameraSet ? (
            <SecondaryButton aria-label="Reset camera" onClick={resetCamera}>
              Reset
            </SecondaryButton>
          ) : undefined
        }
        title="Default camera"
      >
        <Hint>
          {cameraSet
            ? 'Where the map opens when the page gives no position.'
            : 'Not set: the map opens at 0,0 z0'}
        </Hint>
        <div className="grid grid-cols-2 gap-x-2 gap-y-2">
          <CameraField label="Longitude">
            <NumberInput
              label="Longitude"
              max={180}
              min={-180}
              onChange={(next) => setCenter(next ?? 0, undefined)}
              placeholder={0}
              step={0.0001}
              suffix="°"
              value={lng}
            />
          </CameraField>
          <CameraField label="Latitude">
            <NumberInput
              label="Latitude"
              max={90}
              min={-90}
              onChange={(next) => setCenter(undefined, next ?? 0)}
              placeholder={0}
              step={0.0001}
              suffix="°"
              value={lat}
            />
          </CameraField>
          <CameraField label="Zoom">
            <NumberInput
              label="Zoom"
              max={24}
              min={0}
              onChange={(next) => setNumber('zoom', next)}
              placeholder={0}
              step={0.1}
              value={style.zoom}
            />
          </CameraField>
          <CameraField label="Bearing">
            <NumberInput
              label="Bearing"
              max={180}
              min={-180}
              onChange={(next) => setNumber('bearing', next)}
              placeholder={0}
              step={1}
              suffix="°"
              value={style.bearing}
            />
          </CameraField>
          <CameraField label="Pitch">
            <NumberInput
              label="Pitch"
              max={85}
              min={0}
              onChange={(next) => setNumber('pitch', next)}
              placeholder={0}
              step={1}
              suffix="°"
              value={style.pitch}
            />
          </CameraField>
          <CameraField label="Roll">
            <NumberInput
              label="Roll"
              max={180}
              min={-180}
              onChange={(next) => setNumber('roll', next)}
              placeholder={0}
              step={1}
              suffix="°"
              value={style.roll}
            />
          </CameraField>
        </div>
        <div className="flex flex-wrap gap-2">
          <PrimaryButton disabled={!map} onClick={copyCurrentView}>
            <Crosshair className="size-3.5" />
            Use current map view
          </PrimaryButton>
          <SecondaryButton disabled={!map || !cameraSet} onClick={flyToStyleView}>
            <Plane className="size-3.5" />
            Fly to style view
          </SecondaryButton>
        </div>
      </SettingsGroup>
    </SettingsSection>
  );
}

function CameraField({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <span className="text-xs text-foreground">{label}</span>
      {children}
    </div>
  );
}
