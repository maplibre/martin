---
icon: simple/postgresql
tags:
  - postgresql
  - configuration
  - ssl
---

# PostgreSQL Connections

Martin supports standard PostgreSQL connection string settings including `host`, `port`, `user`, `password`, `dbname`, `sslmode`, `connect_timeout`, `keepalives`, `keepalives_idle`, etc.
See the [PostgreSQL docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNSTRING) for more details.

### SSL Connections

Martin supports PostgreSQL `sslmode` settings: `disable`, `prefer`, `require`, `verify-ca` and `verify-full`.
See the [PostgreSQL docs](https://www.postgresql.org/docs/current/libpq-ssl.html) for mode descriptions.
Certificates can be provided in the configuration file (`ssl_cert`, `ssl_key`, `ssl_root_cert`) or on the command line (`--ssl-cert`, `--ssl-key`, `--ca-root-file`).
Command line certificates apply to all PostgreSQL connections.
Martin no longer picks up `psql`'s `PGSSLCERT`, `PGSSLKEY` and `PGSSLROOTCERT` variables by itself -- see [environment vars](../env-vars.md) for how to migrate.

By default, `sslmode` is `prefer` - encrypt (don't check certificates) if the server supports it, but the connection proceeds without SSL if not supported.
This matches `psql` default behavior.

If you require guarantees regarding [eavesdropping](https://en.wikipedia.org/wiki/Eavesdropping) or [MITM protection](https://en.wikipedia.org/wiki/Man-in-the-middle_attack), you need a different option.
Use the `sslmode` parameter to specify a different mode:

```bash
martin postgres://user:password@host/db?sslmode=verify-full
```

For a practical walkthrough of SSL certificate setup - including creation, configuration, and troubleshooting - see our [PostgreSQL SSL Certificates Recipe](../pg-ssl-certificates.md).
