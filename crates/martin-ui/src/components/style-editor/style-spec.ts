import {
  expression,
  type LayerSpecification,
  latest,
  type StylePropertySpecification,
  type StyleSpecification,
  function as styleFunction,
  validateStyleMin,
} from '@maplibre/maplibre-gl-style-spec';

/** Every layer `type` the v8 spec knows. */
export type LayerType = LayerSpecification['type'];

/** The two property blocks of a layer. */
export type PropertyGroup = 'paint' | 'layout';

/** The parts of a v8 property definition the editor reads. */
export interface PropertySpec {
  /** Value type, e.g. `color`, `number`, `enum`, `array`. */
  type: string;
  /** Human description; absent at runtime because the bundled reference strips docs (see `usePropertyDocs`). */
  doc?: string;
  /** Value used when the property is not set. */
  default?: unknown;
  /** Inclusive lower bound for numbers. */
  minimum?: number;
  /** Inclusive upper bound for numbers. */
  maximum?: number;
  /** Unit label, e.g. `pixels`. */
  units?: string;
  /** Allowed values of an `enum`, each with its doc. */
  values?: Record<string, { doc?: string }>;
  /** Element type of an `array`. */
  value?: string;
  /** Fixed length of an `array`. */
  length?: number;
  /** `data-driven`, `data-constant`, `constant`, `color-ramp` or `cross-faded(-data-driven)`. */
  'property-type'?: string;
  /** Which expression inputs the property accepts. */
  expression?: { interpolated: boolean; parameters: string[] };
  /** Whether the spec allows a transition. */
  transition?: boolean;
}

/** One property of a layer type together with its definition. */
export interface PropertyEntry {
  /** Property name, e.g. `line-color`. */
  name: string;
  /** Group the property lives in. */
  group: PropertyGroup;
  /** Its v8 definition. */
  spec: PropertySpec;
}

/** Addresses one property of one layer. */
export interface PropertyRef {
  /** Layer id. */
  layer: string;
  /** Group the property lives in. */
  group: PropertyGroup;
  /** Property name. */
  name: string;
}

function table(type: LayerType, group: PropertyGroup): Record<string, PropertySpec> {
  return latest[`${group}_${type}`] as Record<string, PropertySpec>;
}

/** All properties of a layer type in spec order, layout first as the spec lists them. */
export function layerProperties(type: LayerType, group: PropertyGroup): PropertyEntry[] {
  return Object.entries(table(type, group)).map(([name, spec]) => ({ group, name, spec }));
}

/** The v8 definition of one property, or `undefined` when the layer type has no such property. */
export function propertySpec(
  type: LayerType,
  group: PropertyGroup,
  name: string,
): PropertySpec | undefined {
  return table(type, group)[name];
}

/** The current value block of a layer, empty when unset. */
export function groupValues(
  layer: LayerSpecification,
  group: PropertyGroup,
): Record<string, unknown> {
  return ((layer as { paint?: object; layout?: object })[group] ?? {}) as Record<string, unknown>;
}

/** Whether the property can vary with zoom. */
export function acceptsZoom(spec: PropertySpec): boolean {
  return spec.expression?.parameters.includes('zoom') ?? false;
}

/** Whether the property can vary with feature attributes. */
export function acceptsData(spec: PropertySpec): boolean {
  return (
    spec['property-type'] === 'data-driven' || spec['property-type'] === 'cross-faded-data-driven'
  );
}

/** Whether values between stops are interpolated rather than stepped. */
export function isInterpolated(spec: PropertySpec): boolean {
  return spec.expression?.interpolated ?? false;
}

/** Spec errors for one property value, empty when the value is valid; legacy functions are checked after conversion. */
export function validateProperty(value: unknown, name: string, spec: PropertySpec): string[] {
  if (value === undefined) return [];
  if (spec.type === 'enum' && spec.values && typeof value === 'string' && !(value in spec.values)) {
    return [`${name}: expected one of ${Object.keys(spec.values).join(', ')}, found "${value}"`];
  }
  // A bare array without an operator is a literal (`[3, 3]` dasharray); the expression parser needs it wrapped.
  const input = styleFunction.isFunction(value)
    ? styleFunction.convertFunction(value, spec as StylePropertySpecification)
    : Array.isArray(value) && !expression.isExpression(value)
      ? ['literal', value]
      : value;
  const result = expression.createPropertyExpression(
    input,
    name,
    spec as StylePropertySpecification,
  );
  return result.result === 'error'
    ? result.value.map((e) => (e.key ? `${e.key}: ${e.message}` : e.message))
    : [];
}

/** Converts a legacy `{stops}` function to the equivalent expression, returning other values unchanged. */
export function migrateFunction(value: unknown, spec: PropertySpec): unknown {
  return styleFunction.isFunction(value)
    ? styleFunction.convertFunction(value, spec as StylePropertySpecification)
    : value;
}

/** Whether a value is a v8 expression, judged by its operator. */
export function isExpression(value: unknown): boolean {
  return expression.isExpression(value);
}

/** Whether a value is an expression or legacy function rather than a literal. */
export function isComputed(value: unknown): boolean {
  return expression.isExpression(value) || styleFunction.isFunction(value);
}

/** Validation messages for the whole style. */
export function validateStyle(style: StyleSpecification): string[] {
  return validateStyleMin(style).map((e) => e.message);
}

/** Layer types that can draw a source of the given type, per the v8 spec; `undefined` means no source (background). */
export function layerTypesFor(sourceType: string | undefined): LayerType[] {
  switch (sourceType) {
    case undefined:
      return ['background'];
    case 'vector':
    case 'geojson':
      return ['fill', 'line', 'symbol', 'circle', 'heatmap', 'fill-extrusion'];
    case 'raster':
    case 'image':
    case 'video':
      return ['raster'];
    case 'raster-dem':
      return ['hillshade', 'color-relief'];
    default:
      return [];
  }
}

/** Whether a layer type reads features from named source layers, which only vector sources have. */
export function usesSourceLayer(sourceType: string | undefined): boolean {
  return sourceType === 'vector';
}
