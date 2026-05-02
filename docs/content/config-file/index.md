# Configuration File

If you don't want to expose all of your tables and functions, you can list your sources in a configuration file. To
start Martin with a configuration file you need to pass a path to a file with a `--config` argument. Config files may
contain environment variables, which will be expanded before parsing. For example, to use `MY_DATABASE_URL` in your
config file: `connection_string: ${MY_DATABASE_URL}`, or with a
default `connection_string: ${MY_DATABASE_URL:-postgres://postgres@localhost/db}`

```bash
martin --config config.yaml
```

You may wish to auto-generate a config file with `--save-config` argument. This will generate a config yaml file with
all of your configuration, which you can edit to remove any sources you don't want to expose.

```bash
martin  ... ... ...  --save-config config.yaml
```

## Postprocessing

The `process` block tunes encoder settings used by Martin's postprocessing pipeline.
It does **not** enable any conversion - the client drives that via the `Accept` header (e.g. `Accept: application/vnd.maplibre-tile` triggers MVT->MLT).
The `process` block only changes *how* a conversion encodes when it fires.

Currently configurable:

- **MLT encoder** - encoder settings for MVT→MLT conversion.
See the [MLT usage guide](using-guides/mlt.md) for tuning options.

The `process` block can appear at three levels.
The most specific level wins entirely (no merging between levels):

1. **Global** - applies to all sources
2. **Source-type** - applies to all sources of that type (e.g. all PMTiles sources)
3. **Per-source** - applies to a single source

```yaml
# Global: default MLT encoder settings for any source whose tiles get converted
convert-to-mlt: auto

postgres:
  connection_string: postgresql://localhost/mydb
  # Source-type: override the encoder config for all PG sources
  convert-to-mlt: auto
  tables:
    my_table:
      # Per-source: this table uses the default MLT encoder config
      convert-to-mlt: auto
mbtiles: # gets global default
  - some/file.mbtiles
```

## Config Example

--8<-- "files/generated_config.md"

## Validating your config

Martin publishes a JSON Schema for the config file at
[`schemas/config.json`](https://github.com/maplibre/martin/blob/main/schemas/config.json).
You can use it to catch typos, wrong types, and unknown keys before
starting Martin.

### In your editor

Add the directive at the top of your `config.yaml`:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/maplibre/martin/main/schemas/config.json
```

Editors that respect it (any with the
[YAML Language Server](https://github.com/redhat-developer/yaml-language-server)
behind them) will validate keys, types and enums as you type, and offer
autocomplete from the schema.

### From the command line

The same check Martin's CI runs against its own fixtures works on your
config too. With [`uv`](https://docs.astral.sh/uv/) installed:

```bash
uvx --from check-jsonschema check-jsonschema \
    --schemafile https://raw.githubusercontent.com/maplibre/martin/main/schemas/config.json \
    config.yaml
```

A passing run prints `ok -- validation done`; a failing one points at
the offending path with the reason.
