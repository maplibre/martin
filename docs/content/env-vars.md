---
icon: material/variable
tags:
  - configuration
  - deployment
---

# Environment Variables

Martin takes its configuration from [command line parameters](run-with-cli.md) and
[configuration files](config-file/index.md) only.

It does **not** configure itself from environment variables that happen to be set in its
environment. Automatically picking up variables such as `DATABASE_URL` made Martin's behaviour
depend on whatever else the host had exported -- some platforms set `DATABASE_URL` for you, which
made a configuration file impossible to use.

Environment variables are still fully supported where they are unambiguous: **inside a
configuration file**, where you name the variable yourself.

## Using environment variables in a config file

Any value in a config file can reference an environment variable:

```yaml
postgres:
  # fails to start if MY_DATABASE_URL is not set
  connection_string: ${MY_DATABASE_URL}
```

A default can be supplied with `${VAR:-default}`:

```yaml
postgres:
  connection_string: ${MY_DATABASE_URL:-postgresql://postgres@localhost/db}
```

See the [configuration section](config-file/index.md) for the full substitution syntax.

## Migrating away from the automatic variables

Martin used to read the variables below on its own. It now ignores them, and prints a warning at
startup naming any of them that is still set, together with its replacement. The warning never
prints the value of a variable, so connection strings and certificate paths stay out of the logs.

| No longer read  | Command line                    | Config file                                   |
|-----------------|---------------------------------|-----------------------------------------------|
| `DATABASE_URL`  | `martin "$DATABASE_URL"`        | `postgres.connection_string: ${DATABASE_URL}`  |
| `DEFAULT_SRID`  | `--default-srid 4326`           | `postgres.default_srid: ${DEFAULT_SRID}`       |
| `PGSSLCERT`     | `--ssl-cert ./postgresql.crt`   | `postgres.ssl_cert: ${PGSSLCERT}`              |
| `PGSSLKEY`      | `--ssl-key ./postgresql.key`    | `postgres.ssl_key: ${PGSSLKEY}`                |
| `PGSSLROOTCERT` | `--ca-root-file ./root.crt`     | `postgres.ssl_root_cert: ${PGSSLROOTCERT}`     |

### With the command line

Old:

```bash
export DATABASE_URL=postgresql://postgres@localhost/db
export DEFAULT_SRID=4326
martin
```

New -- pass the connection string as an argument:

```bash
martin --default-srid 4326 "$DATABASE_URL"
```

### With a config file

Old -- if `config.yaml` did not define a `postgres` source itself, `DATABASE_URL` silently added
one anyway:

```bash
export DATABASE_URL=postgresql://postgres@localhost/db
martin --config config.yaml
```

New -- name the variable in the config file:

```yaml
# config.yaml
postgres:
  connection_string: ${DATABASE_URL}
  default_srid: 4326
```

```bash
martin --config config.yaml
```

## Variables Martin still reacts to

These are not Martin settings, and are unaffected by the above:

| Environment var           | Description                                                                                                                                                                |
|---------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `RUST_LOG`                | Logging level, e.g. `RUST_LOG=debug` or `RUST_LOG=martin=debug`                                                                                                             |
| `RUST_LOG_FORMAT`         | Log output format: `json`, `full`, `compact` (default), `bare` or `pretty`                                                                                                  |
| `AWS_LAMBDA_RUNTIME_API`  | Set by the AWS Lambda runtime itself. If present, Martin serves requests through Lambda instead of its own HTTP server. See [Running in AWS Lambda](run-with-lambda.md)      |
| `AWS_*`                   | Read by the deprecated `pmtiles` S3 compatibility shim, which warns and points at the equivalent [`pmtiles` config keys](sources-pmtiles.md)                                |
