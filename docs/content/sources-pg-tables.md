---
icon: simple/postgresql
tags:
  - postgresql
  - tile-sources
  - configuration
---

# PostgreSQL Table Sources

A Table Source is a database table or view which can be used to query [vector tiles](https://github.com/mapbox/vector-tile-spec). If a [PostgreSQL connection string](pg-connections/index.md) is given, Martin will publish all tables as data sources if they have at least one geometry column.
If geometry column SRID is 0, a default SRID must be set, or else that geo-column/table will be ignored.
All non-geometry table columns will be published as vector tile feature tags (properties).

## Modifying Tilejson

Martin will automatically generate a `TileJSON` manifest for each table source.
It will contain the `name`, `description`, `minzoom`, `maxzoom`, `bounds` and `vector_layer` information.
For example, if there is a table `public.table_source`:
 the default `TileJSON` might look like this (note that URL will be automatically adjusted to match the request host):

The table:

```sql
CREATE TABLE "public"."table_source" (
  "gid" int4 NOT NULL,
  "geom" "public"."geometry"
);
```

The TileJSON:

```json
{
    "tilejson": "3.0.0",
    "tiles": [
        "http://localhost:3000/table_source/{z}/{x}/{y}"
    ],
    "vector_layers": [
        {
            "id": "table_source",
            "fields": {
                "gid": "int4"
            }
        }
    ],
    "bounds": [
        -2.0,
        -1.0,
        142.84131509869133,
        45.0
    ],
    "description": "public.table_source.geom",
    "name": "table_source"
}
```

By default the `description` and `name` is database identifies about this table, and the bounds is queried from database.
You can fine tune these by adjusting `auto_publish` section in [configuration file](config-file/index.md#full-configuration).

## Filtering rows

A table source serves every row whose geometry intersects the tile.
Add a `filter` to serve only the rows that match it.
The filter is written in [CQL2 text](https://docs.ogc.org/is/21-065r2/21-065r2.html), the OGC Common Query Language, and Martin translates it to SQL when it starts.
A filter that does not parse stops Martin at startup with the reason.

```yaml
postgres:
  tables:
    big_cities:
      schema: public
      table: cities
      srid: 4326
      geometry_column: geom
      filter: population > 100000 AND name NOT LIKE 'Old %'
      properties:
        name: text
```

The filter also applies when Martin computes the bounds of the source.
Column names are CQL2 identifiers, so a mixed-case column is written in double quotes.

## Postprocessing

Table sources support `convert_to_mlt` and `convert_to_mvt` keys to control tile postprocessing.
This can be set for all PostgreSQL sources or for an individual table.
See [Postprocessing](postprocessing/index.md) for details.

```yaml
postgres:
  connection_string: postgresql://localhost/mydb
  tables:
    my_table:
      convert_to_mlt: auto
      convert_to_mvt: auto
```

## TileJSON in SQL Comments

Other than adjusting `auto_publish` section in configuration file, you can fine tune the `TileJSON` on the database side directly: Add a valid JSON as an SQL comment on the table.

Martin will merge table comment into the generated TileJSON using JSON Merge patch.
The following example update description and adds attribution, version, foo(even a nested DIY field) fields to the TileJSON.

```sql
DO $do$ BEGIN
    EXECUTE 'COMMENT ON TABLE table_source IS $tj$' || $$
    {
        "version": "1.2.3",
        "attribution": "osm",
        "description": "a description from table comment",
        "foo": {"bar": "foo"}
    }
    $$::json || '$tj$';
END $do$;
```
