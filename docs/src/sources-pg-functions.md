# PostgreSQL Function Sources

Function Source is a database function which can be used to
query [vector tiles](https://github.com/mapbox/vector-tile-spec). When started, Martin will look for the functions with
a suitable signature.

A function can be used as a Function Source if it returns either a `bytea` value, or a record with `bytea` and a `text` values.  The `text` value is expected to be a user-defined hash, e.g. an MD5 value, and it will eventually be used as an [ETag](https://developer.mozilla.org/de/docs/Web/HTTP/Reference/Headers/ETag).

A valid function must also have these arguments:

| Argument                     | Type    | Description             |
|------------------------------|---------|-------------------------|
| `z` (or `zoom`)              | integer | Tile zoom parameter     |
| `x`                          | integer | Tile x parameter        |
| `y`                          | integer | Tile y parameter        |
| `query` (optional, any name) | json    | Query string parameters |

### Simple Function with coordinate projection

For example, if you have a table with arbitrary geometry `table_source` in WGS84 (`4326` SRID).
If we need the tables' row `field_color` and geometry `geom` as a function source, then it can be written as:

```sql
CREATE OR REPLACE
    FUNCTION function_zxy(z integer, x integer, y integer)
    RETURNS bytea AS $$
DECLARE
  mvt bytea;
BEGIN
  SELECT INTO mvt ST_AsMVT(tile, 'function_zxy', 4096, 'geom') FROM (
    SELECT
      ST_AsMVTGeom(
          ST_Transform(ST_CurveToLine(geom), 3857),
          ST_TileEnvelope(z, x, y),
          4096, 64, true) AS geom,
        field_color AS color
    FROM table_source
    WHERE geom && ST_Transform(ST_TileEnvelope(z, x, y), 4326)
  ) as tile WHERE geom IS NOT NULL;

  RETURN mvt;
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;
```

!!! tip
    > By default, [`ST_TileEnvelope`](https://postgis.net/docs/ST_TileEnvelope.html) produces `3857` SRID and [`ST_AsMVTGeom`](https://postgis.net/docs/ST_AsMVTGeom.html) consumes `3857` SRID.
    > Many tooling (for example [`osm2pgsql`](https://osm2pgsql.org/)) thus directly store their data in `3857` SRID for lower processing overhead.
    > If your data is in `3857` SRID, you can remove the two `ST_Transform` calls.

Lets explain a few of the aspects of the function:

`ST_Transform(ST_CurveToLine(geom), 3857)`

- Since the table in the example can contain arbitrary geometries, we need to transform `CIRCULARSTRING` geometry types.
  Concretely, we use `ST_CurveToLine` to convert a
  - CIRCULAR STRING to regular LINESTRING,
  - CURVEPOLYGON to POLYGON or
  - MULTISURFACE to MULTIPOLYGON.
- `ST_Transform` is necessary as `ST_CurveToLine` returns a geometry in `4326` SRID, which is the SRID of stored geometry in the example.

`WHERE geom && ST_Transform(ST_TileEnvelope(z, x, y), 4326)`

- `&&` is the spatial intersection operator. Thus it checks if the geometry intersects with the tile envelope and uses spatial indexes.
- `ST_Transform` is used to transform the tile envelope from `3857` SRID to `4326` SRID, as `geom` in our example is in `4326` SRID.

!!! note
    > The planning mode `IMMUTABLE STRICT PARALLEL SAFE` allows postgres further freedom to optimize our function.
    > Your function is likely to be the same category as the example, but be careful to not cause unexpected behavior.
    >
    > - [`IMMUTABLE`](https://www.postgresql.org/docs/current/sql-createfunction.html#:~:text=existing%20function%20definition.-,IMMUTABLE,-STABLE%0AVOLATILE)
    >   The function does not have side effects.
    >
    >   > Indicates that the function cannot modify the database and always returns the same result when given the same argument values;
    >   > that is, it does not do database lookups or otherwise use information not directly present in its argument list.
    >   > If this option is given, any call of the function with all-constant arguments can be immediately replaced with the function value.
    > - `STRICT`: Our function will not be called if any of the arguments are `NULL`.
    > - [`PARALLEL SAFE`](https://www.postgresql.org/docs/current/parallel-safety.html):
    >   Our function is safe to call in parallel as it does not modify the database, nor use randomness or temporary tables.
    >
    >   > Functions should be labeled parallel unsafe if they
    >   > - modify any database state,
    >   > - change the transaction state (other than by using a subtransaction for error recovery),
    >   > - access sequences (e.g., by calling currval) or
    >   > - make persistent changes to settings.
    >   >
    >   > They should be labeled parallel restricted if they
    >   > - access temporary tables,
    >   > - client connection state,
    >   > - cursors,
    >   > - prepared statements, or
    >   > - miscellaneous backend-local state which the system cannot synchronize in parallel mode
    >   >   (e.g., setseed cannot be executed other than by the group leader because a change made by another process
    >   >    would not be reflected in the leader).
    >   >
    >   > In general, if a function is labeled as being safe when it is restricted or unsafe, or if it is labeled as being restricted
    >   > when it is in fact unsafe, it may throw errors or produce wrong answers when used in a parallel query.
    >   > C-language functions could in theory exhibit totally undefined behavior if mislabeled, since there is no way for the system
    >   > to protect itself against arbitrary C code, but in most likely cases the result will be no worse than for any other function.
    >   > If in doubt, functions should be labeled as UNSAFE, which is the default.

### Function with Query Parameters

Users may add a `query` parameter to pass additional parameters to the function.

The `query_params` argument is a JSON representation of the tile request query params. Query params could be passed as
simple query values, e.g.

```bash
curl localhost:3000/function_zxy_query/0/0/0?answer=42
```

You can also
use [urlencoded](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/encodeURIComponent)
params to encode complex values:

```bash
curl \
  --data-urlencode 'arrayParam=[1, 2, 3]' \
  --data-urlencode 'numberParam=42' \
  --data-urlencode 'stringParam=value' \
  --data-urlencode 'booleanParam=true' \
  --data-urlencode 'objectParam={"answer" : 42}' \
  --get localhost:3000/function_zxy_query/0/0/0
```

then `query_params` will be parsed as:

```json
{
  "arrayParam": [1, 2, 3],
  "numberParam": 42,
  "stringParam": "value",
  "booleanParam": true,
  "objectParam": { "answer": 42 }
}
```

You can access this params using [json operators](https://www.postgresql.org/docs/current/functions-json.html):

```sql
...WHERE answer = (query_params->'objectParam'->>'answer')::int;
```

As an example, our `table_source` in WGS84 (`4326` SRID) has a column `answer` of type `integer`.
The function `function_zxy_query` will return a MVT tile with the `answer` column as a property.

```sql
CREATE OR REPLACE
    FUNCTION function_zxy_query(z integer, x integer, y integer, query_params json)
    RETURNS bytea AS $$
DECLARE
  mvt bytea;
BEGIN
  SELECT INTO mvt ST_AsMVT(tile, 'function_zxy_query', 4096, 'geom') FROM (
    SELECT
      ST_AsMVTGeom(
          ST_Transform(ST_CurveToLine(geom), 3857),
          ST_TileEnvelope(z, x, y),
          4096, 64, true) AS geom
    FROM table_source
    WHERE geom && ST_Transform(ST_TileEnvelope(z, x, y), 4326) AND
          answer = (query_params->>'answer')::int
  ) as tile WHERE geom IS NOT NULL;

  RETURN mvt;
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;
```

### Modifying TileJSON

Martin will automatically generate a basic [TileJSON](https://github.com/mapbox/tilejson-spec) manifest for each
function source.
This will contain the `name` and `description` of the function, plus optionally `minzoom`, `maxzoom`, and `bounds`
(if they were specified via one of the configuration methods).

For example, if there is a function `public.function_zxy_query_jsonb`, the default `TileJSON` might look like this:

```json
{
  "tilejson": "3.0.0",
  "tiles": [
    "http://localhost:3111/function_zxy_query_jsonb/{z}/{x}/{y}"
  ],
  "name": "function_zxy_query_jsonb",
  "description": "public.function_zxy_query_jsonb"
}
```

!!! note
    > The URL will be automatically adjusted to match the request host

#### TileJSON in SQL Comments

To modify automatically generated `TileJSON`, you can add a valid JSON as an SQL comment on the function.
Martin will merge function comment into the generated `TileJSON` using [JSON Merge patch](https://www.rfc-editor.org/rfc/rfc7386).
The following example adds `attribution` and `version` fields to the `TileJSON`.

!!! note
    > This example uses `EXECUTE` to ensure that the comment is a valid JSON
    > (or else PostgreSQL will throw an error).
    > You can use other methods of creating SQL comments.

```sql
DO $do$ BEGIN
    EXECUTE 'COMMENT ON FUNCTION my_function_name IS $tj$' || $$
    {
        "description": "my new description",
        "attribution": "my attribution",
        "vector_layers": [
            {
                "id": "my_layer_id",
                "fields": {
                    "field1": "String",
                    "field2": "Number"
                }
            }
        ]
    }
    $$::json || '$tj$';
END $do$;
```
