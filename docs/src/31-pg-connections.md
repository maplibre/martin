# PostgreSQL Connection String

Martin supports many of the PostgreSQL connection string settings such as `host`, `port`, `user`, `password`, `dbname`, `sslmode`, `connect_timeout`, `keepalives`, `keepalives_idle`, etc. See the [PostgreSQL docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNSTRING) for more details.

## PostgreSQL SSL Connections

Martin supports PostgreSQL `sslmode` including `disable`, `prefer`, `require`, `verify-ca` and `verify-full` modes as described in the [PostgreSQL docs](https://www.postgresql.org/docs/current/libpq-ssl.html).  Certificates can be provided in the configuration file, or can be set using the same env vars as used for `psql`. When set as env vars, they apply to all PostgreSQL connections.  See [environment vars](21-env-vars.md) section for more details.

By default, `sslmode` is set to `prefer` which means that SSL is used if the server supports it, but the connection is not aborted if the server does not support it.  This is the default behavior of `psql` and is the most compatible option.  Use the `sslmode` param to set a different `sslmode`, e.g. `postgresql://user:password@host/db?sslmode=require`.
