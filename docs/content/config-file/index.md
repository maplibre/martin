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

## Config Example

```yaml
--8<-- "config.yaml"
```
