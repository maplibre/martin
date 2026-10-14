The filter is written in CQL2, the OGC Common Query Language, using the comparison, logical, text, temporal and spatial operators of [its text encoding](https://docs.ogc.org/is/21-065r2/21-065r2.html).

Martin translates it to SQL when it starts and adds it to the tile query, so a filter that does not parse stops Martin at startup with the reason.
The filter also applies when Martin computes the bounds of the source.
Column names are used exactly as written, so `Population` names the column `Population` and not `population`.
A name with spaces or other special characters goes in double quotes.

=== "Comparing values"

    Compare a column with a literal using `=`, `<>`, `<`, `<=`, `>` and `>=`.
    Strings go in single quotes, and numbers and `true`/`false` are written as they are.
    `BETWEEN`, `IN` and `IS NULL` cover ranges, lists and missing values.

    ```yaml
    # Cities with a population of ten to a hundred thousand
    filter: population BETWEEN 10000 AND 100000
    # Rows with one of these kinds
    filter: kind IN ('city', 'town')
    # Rows that have a name
    filter: name IS NOT NULL
    # Rows whose boolean column is set
    filter: is_capital = true
    ```

=== "Combining conditions"

    Join conditions with `AND`, `OR` and `NOT`, and group them with parentheses.

    ```yaml
    # Large cities and towns
    filter: population > 100000 AND (kind = 'city' OR kind = 'town')
    # Everything but villages
    filter: NOT (kind = 'village')
    ```

=== "Matching text"

    `LIKE` matches a pattern in which `%` stands for any run of characters and `_` for a single one.
    Put a `\` before either to match it literally.
    `CASEI` compares text without regard to case.

    ```yaml
    # Names starting with "San "
    filter: name LIKE 'San %'
    # Names ending in "burg", except Hamburg
    filter: name LIKE '%burg' AND name <> 'Hamburg'
    # "Berlin", "berlin" and "BERLIN"
    filter: CASEI(name) = 'berlin'
    ```

=== "Dates and times"

    Write a date as `DATE('2024-01-01')` and an instant as `TIMESTAMP('2024-01-01T00:00:00Z')`.
    Compare them like numbers, or use a temporal operator such as `T_BEFORE`, `T_AFTER` or `T_DURING`.
    Those take an `INTERVAL('start', 'end')` in which `'..'` leaves an end open.

    ```yaml
    # Rows updated since the start of 2024
    filter: updated_at >= DATE('2024-01-01')
    # Rows updated within a period
    filter: T_DURING(updated_at, INTERVAL('2024-01-01T00:00:00Z', '2024-12-31T23:59:59Z'))
    ```

=== "Spatial and other functions"

    `S_INTERSECTS`, `S_WITHIN`, `S_CONTAINS`, `S_DISJOINT` and the other spatial operators compare the geometry column with a `BBOX(xmin, ymin, xmax, ymax)` or a WKT literal such as `POLYGON((...))`.
