# Martin Demo

## Usage in Docker-Compose

The frontend is built from `frontend/Dockerfile` (multi-stage: pnpm build, then nginx serves the static output). Ensure you have the Dockerfile and `frontend/nginx.conf` in place before running:

```shell
just up
```

Then open https://localhost in your browser.

The Docker database is initialized with NYC taxi data (for the **get_trips** layer) so the demo map works against the local Martin instance. The map uses the local Oberbayern PMTiles basemap plus the get_trips overlay. The frontend is built with `PUBLIC_MARTIN_BASE_URL=/api/martin` and nginx proxies `/api/martin/` to the tiles service.

## Demo features

The demo showcases Martin’s live vector tile generation:

- **Map**: MapLibre GL JS renders the Oberbayern basemap and vector tiles from the configured Martin instance. The primary demo layer is **NYC taxi trips** (get_trips), a parameterized PostGIS function driven by the **demo-layers** content collection.
- **Parameterized filters**: The get_trips layer declares `allowedParameters` (e.g. `date_from`, `date_to`, `hour`). The UI sends these only as URL query parameters; no raw SQL is exposed to users.
- **Metrics panel**: The demo fetches `/_/metrics` from the Martin base URL and shows tile request count and average duration.

## Martin backend for parameterized tiles

To use **parameterized** layers (e.g. get_trips with date/hour filters), the Martin instance must expose **Postgres function sources** that accept `query_params json`. The frontend only appends safe query strings to tile URLs (e.g. `?date_from=2017-01-01&date_to=2017-01-31&hour=-1`). Filtering is applied in the database function; the demo never sends arbitrary SQL.

Example function signature:

```sql
CREATE FUNCTION get_trips(z integer, x integer, y integer, query_params json)
RETURNS bytea
```

The function reads filter values from `query_params` (e.g. `query_params->>'date_from'`) and uses them in the query. See the [Martin docs on function sources with query parameters](https://maplibre.org/martin/sources-pg-functions.html#function-with-query-parameters).

## Configuration

- **Martin base URL**: Set `PUBLIC_MARTIN_BASE_URL` at build time so tiles and metrics use your Martin instance (e.g. when running with `just up`). If unset, the demo uses `https://martin.maplibre.org`.
- **CORS**: If the frontend is served from a different origin than Martin, ensure Martin allows the demo origin so that tile and metrics requests succeed.
- **Optional env**: Copy `frontend/.env.example` to `frontend/.env.local` and set `GITHUB_TOKEN` if you want higher rate limits when the build fetches repo stats (stars, contributors, latest release). Defaults are fine for martin.maplibre.org.
