## Table Sources

Table Source is a database table which can be used to query [vector tiles](https://github.com/mapbox/vector-tile-spec). If a [PostgreSQL connection string](pg-connections.md) is given, Martin will publish all tables as data sources if they have at least one geometry column. If geometry column SRID is 0, a default SRID must be set, or else that geo-column/table will be ignored. All non-geometry table columns will be published as vector tile feature tags (properties).

### Modifying Tilejson

Martin will automatically generate a `TileJSON` manifest for each table source. It will contain the `name`, `description`, `minzoom`, `maxzoom`, `bounds` and `vector_layer` information.
For example, if there is a table `public.table_source`:
 the default `TileJSON` might look like this (note that URL will be automatically adjusted to match the request host):

The table:

```sql
CREATE TABLE "public"."table_source" ( "gid" int4 NOT NULL, "geom" "public"."geometry" );
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

By default the `description` and `name` is database identifies about this table, and the bounds is queried from database. You can fine tune these by adjusting `auto_publish` section in [configuration file](https://maplibre.org/martin/config-file.html#config-example).

#### TileJSON in SQL Comments

Other than adjusting `auto_publish` section in configuration file, you can fine tune the `TileJSON` on the database side directly: Add a valid JSON as an SQL comment on the table.

Martin will merge table comment into the generated TileJSON using JSON Merge patch. The following example update description and adds attribution, version, foo(even a nested DIY field) fields to the TileJSON.

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
