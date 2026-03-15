/** Escapes regex metacharacters in a string so it can be used in RegExp. */
function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/**
 * Format a value for SQL display: numbers as-is, strings single-quoted and escaped.
 */
function formatSqlValue(value: string | number): string {
  if (typeof value === 'number') return String(value);
  return `'${String(value).replace(/'/g, "''")}'`;
}

/**
 * Builds the SQL panel display string from template, current hover, and filter state.
 * Used by the merged demo panel so filters and SQL preview stay in sync.
 */
export function getSqlDisplay(
  template: string,
  hoveredName: string | null,
  filterState: Record<string, string | number>,
): string {
  let out = template;
  if (hoveredName) {
    out = out.replace(/\{\{hover\}\}/g, hoveredName);
  }
  for (const [key, value] of Object.entries(filterState)) {
    const placeholder = new RegExp(escapeRegExp(`{{${key}}}`), 'g');
    out = out.replace(placeholder, formatSqlValue(value));
  }
  return out.replace(/\{\{\w+\}\}/g, '');
}
