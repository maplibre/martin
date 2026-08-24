---
icon: material/file-cog
tags:
  - configuration
---

# Configuration

If you don't want to expose all of your tables and functions, you can list your sources in a configuration file.
To start Martin with a configuration file you need to pass a path to a file with a `--config` argument.
Config files may
contain environment variables, which will be expanded before parsing.
For example, to use `MY_DATABASE_URL` in your
config file: `connection_string: ${MY_DATABASE_URL}`, or with a
default `connection_string: ${MY_DATABASE_URL:-postgres://postgres@localhost/db}`

```bash
martin --config config.yaml
```

!!! warning "Deprecation of single-colon interpolation"
    The legacy single-colon default `${MY_DATABASE_URL:postgres://postgres@localhost/db}` is still
    accepted for backward compatibility, but is deprecated in favor of the `:-` form shown above.

!!! tip "auto-generate a config file with `--save-config`"
    You can generate a config yaml file with all of your configuration, which you can edit to remove any sources you don't want to expose.
    
    ```bash
    martin  ... ... ...  --save-config config.yaml
    ```

## Full Configuration

--8<-- "files/generated_config.md"

## Validating your config

Martin publishes a JSON Schema for the config file at
[`schemas/config.json`](https://github.com/maplibre/martin/blob/main/schemas/config.json).
You can use it to catch typos, wrong types, and unknown keys before
starting Martin.

=== "In your editor"

    Add the directive at the top of your `config.yaml`:
    
    ```yaml
    # yaml-language-server: $schema=https://raw.githubusercontent.com/maplibre/martin/main/schemas/config.json
    ```
    
    Editors that respect it (any with the [YAML Language Server](https://github.com/redhat-developer/yaml-language-server) behind them) will validate your config.
    This means you get schema based autocomplete for keys, types and enums as you type.

=== "From the command line"

    The same check Martin's CI runs against its own fixtures works on your config too.
    With [`uv`](https://docs.astral.sh/uv/) installed:
    
    ```bash
    $ uvx --from check-jsonschema check-jsonschema \
        --schemafile https://raw.githubusercontent.com/maplibre/martin/main/schemas/config.json \
        config.yaml
    ```
