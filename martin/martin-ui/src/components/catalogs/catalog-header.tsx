interface CatalogHeaderProps {
  /** What one entry is called, singular then plural, such as `['tile source', 'tile sources']`. */
  noun: readonly [string, string];
  searchQuery: string;
  /** Number of entries after filtering. */
  count: number;
  /** Number of entries before filtering. */
  total: number;
}

/** States how many entries the catalog holds, or how many match the search. */
export function CatalogHeader({ noun, searchQuery, count, total }: CatalogHeaderProps) {
  const filtered = searchQuery !== '' && count !== total;
  return (
    <h2 className="text-lg font-semibold tabular-nums text-foreground">
      {filtered ? (
        <>
          {count} <span className="font-normal text-muted-foreground">of {total}</span>
        </>
      ) : (
        total
      )}{' '}
      {(filtered ? total : count) === 1 ? noun[0] : noun[1]}
    </h2>
  );
}
