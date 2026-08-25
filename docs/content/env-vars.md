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

| Environment var          | Description                                                                                                                                |
|--------------------------|--------------------------------------------------------------------------------------------------------------------------------------------|
| `AWS_LAMBDA_RUNTIME_API` | If defined, connect to AWS Lambda to handle requests. The regular HTTP server is not used. See [Running in AWS Lambda](run-with-lambda.md) |
| `AWS_CONTAINER_CREDENTIALS_RELATIVE_URI`<br/>`AWS_CONTAINER_CREDENTIALS_FULL_URI`<br/>`AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`<br/>`AWS_WEB_IDENTITY_TOKEN_FILE`<br/>`AWS_ROLE_ARN`<br/>`AWS_ROLE_SESSION_NAME`<br/>`AWS_ENDPOINT_URL_STS` | Injected by ECS, Fargate and EKS to say where the task role's credentials come from. Used for S3-backed PMTiles sources unless `pmtiles.profile` or the matching `pmtiles.*` setting is configured. See [PMTiles sources](sources-pmtiles.md) |

!!! warning "Deprecated environment variables"
    Reading **below environment variables is deprecated** and **will be removed** in a **future** (semver major) **release**.
    See [#1052](https://github.com/maplibre/martin/issues/1052) for further context why this was done.
    Use the appropriate CLI flags or our yaml interpolation (`key: ${ENV_VAR}`) support that replaces them.

    | Environment var <br/> Config File key    | Example                                   | Description                                                                                                                                                                                                |
    |------------------------------------------|-------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
    | `DATABASE_URL` <br/> `connection_string` | `postgres://`<br/>`postgres@localhost/db` | Postgres database connection                                                                                                                                                                               |
    | `DEFAULT_SRID` <br/> `default_srid`      | `4326`                                    | If a PostgreSQL table has a geometry column with SRID=0, use this value instead                                                                                                                            |
    | `PGSSLCERT` <br/> `ssl_cert`             | `./postgresql.crt`                        | A file with a client SSL certificate. [docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLCERT)                                                                             |
    | `PGSSLKEY` <br/> `ssl_key`               | `./postgresql.key`                        | A file with the key for the client SSL certificate. [docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLKEY)                                                                |
    | `PGSSLROOTCERT` <br/> `ssl_root_cert`    | `./root.crt`                              | A file with trusted root certificate(s). The file should contain a sequence of PEM-formatted CA certificates. [docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLROOTCERT) |
