import { expression, type StylePropertySpecification } from '@maplibre/maplibre-gl-style-spec';
import type { AttributeInfo } from '../source-schema';
import type { PropertySpec } from '../style-spec';

/** A literal a condition compares against. */
export type FilterScalar = string | number | boolean;

/** The pseudo attribute that stands for the feature's geometry type. */
export const GEOMETRY_TYPE = '$type';

/** The geometry types a `$type` condition can name. */
export const GEOMETRY_TYPES = ['Point', 'LineString', 'Polygon'] as const;

/** Operators that compare against one value. */
export type CompareOperator = 'is' | 'isNot' | 'lt' | 'lte' | 'gt' | 'gte';

/** Operators that compare against a list of values. */
export type SetOperator = 'oneOf' | 'notOneOf';

/** Operators that test only whether the attribute is present. */
export type ExistsOperator = 'exists' | 'notExists';

/** One row of a structured filter. */
export type FilterCondition =
  | {
      /** Attribute name, or `$type` for the geometry type. */
      attribute: string;
      /** How the attribute is compared. */
      operator: CompareOperator;
      /** Value compared against. */
      value: FilterScalar;
    }
  | {
      /** Attribute name, or `$type` for the geometry type. */
      attribute: string;
      /** Whether the attribute must be in or out of the list. */
      operator: SetOperator;
      /** Accepted or rejected values. */
      values: FilterScalar[];
    }
  | {
      /** Attribute name. */
      attribute: string;
      /** Whether the attribute must be present or absent. */
      operator: ExistsOperator;
    };

/** Any operator of a condition. */
export type FilterOperator = FilterCondition['operator'];

/** A filter as the editor presents it. */
export type FilterModel =
  | {
      /** Conditions that must all hold. */
      mode: 'structured';
      /** The conditions, empty when there is no filter. */
      conditions: FilterCondition[];
    }
  | {
      /** The filter is edited as raw JSON. */
      mode: 'expression';
      /** The filter as stored. */
      expression: unknown;
    };

/** Operators in the order the menu lists them. */
export const OPERATORS: readonly FilterOperator[] = [
  'is',
  'isNot',
  'oneOf',
  'notOneOf',
  'exists',
  'notExists',
  'lt',
  'lte',
  'gt',
  'gte',
];

const COMPARE: Record<string, CompareOperator> = {
  '!=': 'isNot',
  '<': 'lt',
  '<=': 'lte',
  '==': 'is',
  '>': 'gt',
  '>=': 'gte',
};

const COMPARE_SYMBOL: Record<CompareOperator, string> = {
  gt: '>',
  gte: '>=',
  is: '==',
  isNot: '!=',
  lt: '<',
  lte: '<=',
};

const ORDERING: CompareOperator[] = ['lt', 'lte', 'gt', 'gte'];

/** Property definition a filter is validated against: a boolean expression of zoom and feature. */
export const FILTER_SPEC: PropertySpec = {
  doc: 'Which features the layer draws.',
  expression: { interpolated: false, parameters: ['zoom', 'feature'] },
  'property-type': 'data-driven',
  type: 'boolean',
};

function isScalar(value: unknown): value is FilterScalar {
  return typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean';
}

function scalars(values: unknown[]): FilterScalar[] | undefined {
  return values.every(isScalar) ? values : undefined;
}

function isKey(value: unknown): value is string {
  return typeof value === 'string' && (value === GEOMETRY_TYPE || !value.startsWith('$'));
}

function isGet(value: unknown): value is ['get', string] {
  return Array.isArray(value) && value.length === 2 && value[0] === 'get' && isKey(value[1]);
}

function isGeometryType(value: unknown): boolean {
  return Array.isArray(value) && value.length === 1 && value[0] === 'geometry-type';
}

/** The attribute a modern subexpression reads, `$type` for `['geometry-type']`. */
function readAttribute(value: unknown): string | undefined {
  if (isGeometryType(value)) return GEOMETRY_TYPE;
  return isGet(value) ? value[1] : undefined;
}

/** Whether an operator makes sense for the attribute: geometry type has no ordering or presence test. */
export function operatorAllowed(attribute: string, operator: FilterOperator): boolean {
  if (attribute !== GEOMETRY_TYPE) return true;
  return (
    operator === 'is' || operator === 'isNot' || operator === 'oneOf' || operator === 'notOneOf'
  );
}

function build(
  attribute: string,
  operator: FilterOperator,
  values: FilterScalar[],
): FilterCondition | undefined {
  if (!operatorAllowed(attribute, operator)) return undefined;
  switch (operator) {
    case 'exists':
    case 'notExists':
      return { attribute, operator };
    case 'oneOf':
    case 'notOneOf':
      return values.length > 0 ? { attribute, operator, values } : undefined;
    default: {
      const [value] = values;
      if (values.length !== 1 || value === undefined) return undefined;
      if (ORDERING.includes(operator) && typeof value !== 'number') return undefined;
      return { attribute, operator, value };
    }
  }
}

function parseLegacy(filter: unknown[]): FilterCondition | undefined {
  const [op, key, ...rest] = filter;
  if (!isKey(key)) return undefined;
  if (op === 'has' && rest.length === 0) return build(key, 'exists', []);
  if (op === '!has' && rest.length === 0) return build(key, 'notExists', []);
  const values = scalars(rest);
  if (!values) return undefined;
  if (op === 'in') return build(key, 'oneOf', values);
  if (op === '!in') return build(key, 'notOneOf', values);
  const compare = typeof op === 'string' ? COMPARE[op] : undefined;
  return compare ? build(key, compare, values) : undefined;
}

function parseMatch(filter: unknown[]): FilterCondition | undefined {
  const [, input, labels, yes, no] = filter;
  const attribute = readAttribute(input);
  if (attribute === undefined || filter.length !== 5 || yes === no) return undefined;
  if (typeof yes !== 'boolean' || typeof no !== 'boolean') return undefined;
  const values = Array.isArray(labels) ? scalars(labels) : scalars([labels]);
  return values ? build(attribute, yes ? 'oneOf' : 'notOneOf', values) : undefined;
}

function parseIn(filter: unknown[], negate: boolean): FilterCondition | undefined {
  const [, input, haystack] = filter;
  const attribute = readAttribute(input);
  if (attribute === undefined || filter.length !== 3) return undefined;
  if (!Array.isArray(haystack) || haystack[0] !== 'literal' || !Array.isArray(haystack[1])) {
    return undefined;
  }
  const values = scalars(haystack[1]);
  return values ? build(attribute, negate ? 'notOneOf' : 'oneOf', values) : undefined;
}

function parseModernCompare(filter: unknown[]): FilterCondition | undefined {
  const [op, left, right] = filter;
  const compare = typeof op === 'string' ? COMPARE[op] : undefined;
  const attribute = readAttribute(left);
  if (!compare || attribute === undefined || filter.length !== 3 || !isScalar(right)) {
    return undefined;
  }
  return build(attribute, compare, [right]);
}

function parseCondition(filter: unknown): FilterCondition | undefined {
  if (!Array.isArray(filter)) return undefined;
  const [op, second] = filter;
  if (op === 'match') return parseMatch(filter);
  if (op === 'in') return typeof second === 'string' ? parseLegacy(filter) : parseIn(filter, false);
  if (op === '!in') return parseLegacy(filter);
  if (op === '!has') return parseLegacy(filter);
  if (op === 'has' && filter.length === 2 && typeof second === 'string') return parseLegacy(filter);
  if (op === '!') {
    const inner = Array.isArray(second) ? second : [];
    if (inner[0] === 'has' && inner.length === 2 && typeof inner[1] === 'string') {
      return build(inner[1], 'notExists', []);
    }
    return inner[0] === 'in' ? parseIn(inner, true) : undefined;
  }
  if (typeof op === 'string' && op in COMPARE) {
    return typeof second === 'string' ? parseLegacy(filter) : parseModernCompare(filter);
  }
  return undefined;
}

/** Reads a layer filter, legacy or modern, into conditions joined by "all", or keeps it as an expression. */
export function parseFilter(filter: unknown): FilterModel {
  if (filter === undefined || filter === null) return { conditions: [], mode: 'structured' };
  const items = Array.isArray(filter) && filter[0] === 'all' ? filter.slice(1) : [filter];
  const conditions = items.map(parseCondition);
  const complete = conditions.filter((c): c is FilterCondition => c !== undefined);
  if (complete.length !== conditions.length) return { expression: filter, mode: 'expression' };
  return { conditions: complete, mode: 'structured' };
}

function reads(attribute: string): unknown[] {
  return attribute === GEOMETRY_TYPE ? ['geometry-type'] : ['get', attribute];
}

function serializeCondition(condition: FilterCondition): unknown {
  const { attribute, operator } = condition;
  if ('values' in condition) {
    const { values } = condition;
    const yes = operator === 'oneOf';
    const strings = values.every((v) => typeof v === 'string');
    const numbers = values.every((v) => typeof v === 'number' && Number.isInteger(v));
    if (strings || numbers) return ['match', reads(attribute), values, yes, !yes];
    const inside = ['in', reads(attribute), ['literal', values]];
    return yes ? inside : ['!', inside];
  }
  if ('value' in condition) {
    return [COMPARE_SYMBOL[condition.operator], reads(attribute), condition.value];
  }
  return operator === 'exists' ? ['has', attribute] : ['!', ['has', attribute]];
}

function complete(condition: FilterCondition): boolean {
  if (condition.attribute === '') return false;
  return 'values' in condition ? condition.values.length > 0 : true;
}

/** Writes conditions as one modern expression; rows still missing an attribute or values are left out, none gives `undefined`. */
export function serializeFilter(model: FilterModel): unknown {
  if (model.mode === 'expression') return model.expression;
  const parts = model.conditions.filter(complete).map(serializeCondition);
  if (parts.length === 0) return undefined;
  return parts.length === 1 ? parts[0] : ['all', ...parts];
}

/** Spec errors of a filter, empty when it is valid; legacy filters are checked after conversion. */
export function validateFilter(filter: unknown): string[] {
  if (filter === undefined || filter === null) return [];
  const model = parseFilter(filter);
  const modern = model.mode === 'structured' ? serializeFilter(model) : filter;
  if (modern === undefined) return [];
  const result = expression.createExpression(
    modern,
    'filter',
    FILTER_SPEC as StylePropertySpecification,
  );
  return result.result === 'error'
    ? result.value.map((e) => (e.key ? `${e.key}: ${e.message}` : e.message))
    : [];
}

/** Turns typed text into the literal the attribute holds: numbers and booleans by observed type, else a string. */
export function coerceValue(text: string, attribute: AttributeInfo | undefined): FilterScalar {
  const number = Number(text);
  const numeric = text.trim() !== '' && Number.isFinite(number);
  if (attribute?.kind === 'number') return numeric ? number : text;
  if (attribute?.kind === 'boolean' && (text === 'true' || text === 'false'))
    return text === 'true';
  if (numeric && attribute?.values.includes(number)) return number;
  return text;
}

/** The condition after switching operator, keeping what still applies of its values. */
export function changeOperator(
  condition: FilterCondition,
  operator: FilterOperator,
): FilterCondition {
  const { attribute } = condition;
  const old =
    'values' in condition ? condition.values : 'value' in condition ? [condition.value] : [];
  if (operator === 'exists' || operator === 'notExists') return { attribute, operator };
  if (operator === 'oneOf' || operator === 'notOneOf') return { attribute, operator, values: old };
  const first = old[0];
  const value = ORDERING.includes(operator) && typeof first !== 'number' ? 0 : (first ?? '');
  return { attribute, operator, value };
}
