---
icon: material/variable
tags:
  - configuration
  - deployment
---

# Environment Variables

You can configure Martin using environment variables, but only if the configuration file is not used.
The configuration file itself can use environment variables if needed.
See [configuration section](config-file/index.md) on how to use environment variables with config files.
See also [SSL configuration](pg-connections/index.md#ssl-connections) section below.

**Deprecated:** reading these five variables implicitly is deprecated and may be removed in a
future release ([#1052](https://github.com/maplibre/martin/issues/1052)). Martin warns once at
startup naming any of them that are still set, together with the config-file key (and CLI flag,
where one exists) that replaces them.

| Environment var <br/> Config File key    | Example                                   | Description                                                                                                                                                                                                |
|------------------------------------------|-------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `DATABASE_URL` <br/> `connection_string` | `postgres://`<br/>`postgres@localhost/db` | Postgres database connection                                                                                                                                                                               |
| `DEFAULT_SRID` <br/> `default_srid`      | `4326`                                    | If a PostgreSQL table has a geometry column with SRID=0, use this value instead                                                                                                                            |
| `PGSSLCERT` <br/> `ssl_cert`             | `./postgresql.crt`                        | A file with a client SSL certificate. [docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLCERT)                                                                             |
| `PGSSLKEY` <br/> `ssl_key`               | `./postgresql.key`                        | A file with the key for the client SSL certificate. [docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLKEY)                                                                |
| `PGSSLROOTCERT` <br/> `ssl_root_cert`    | `./root.crt`                              | A file with trusted root certificate(s). The file should contain a sequence of PEM-formatted CA certificates. [docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLROOTCERT) |

| Environment var <br/> Config File key    | Example                                   | Description                                                                                                                                                                                                |
|------------------------------------------|-------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `AWS_LAMBDA_RUNTIME_API` <br/> -         |                                           | If defined, connect to AWS Lambda to handle requests. The regular HTTP server is not used. See [Running in AWS Lambda](run-with-lambda.md)                                                                 |

!!! warning "Deprecated environemnt variables"
    Reading below environment variables is deprecated and will be removed in a future, major release.
    See [#1052](https://github.com/maplibre/martin/issues/1052)) for further context.
    Use the appropriate CLI flags or our yaml interpolation (`key: ${ENV_VAR}`) support that replaces them.

    | Environment var <br/> Config File key    | Example                                   | Description                                                                                                                                                                                                |
    |------------------------------------------|-------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
    | `DATABASE_URL` <br/> `connection_string` | `postgres://`<br/>`postgres@localhost/db` | Postgres database connection                                                                                                                                                                               |
    | `DEFAULT_SRID` <br/> `default_srid`      | `4326`                                    | If a PostgreSQL table has a geometry column with SRID=0, use this value instead                                                                                                                            |
    | `PGSSLCERT` <br/> `ssl_cert`             | `./postgresql.crt`                        | A file with a client SSL certificate. [docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLCERT)                                                                             |
    | `PGSSLKEY` <br/> `ssl_key`               | `./postgresql.key`                        | A file with the key for the client SSL certificate. [docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLKEY)                                                                |
    | `PGSSLROOTCERT` <br/> `ssl_root_cert`    | `./root.crt`                              | A file with trusted root certificate(s). The file should contain a sequence of PEM-formatted CA certificates. [docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLROOTCERT) |
