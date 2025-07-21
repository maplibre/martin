## PostgreSQL Connections

Martin supports standard PostgreSQL connection string settings including `host`, `port`, `user`, `password`, `dbname`, `sslmode`, `connect_timeout`, `keepalives`, `keepalives_idle`, etc.
See the [PostgreSQL docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNSTRING) for more details.

### SSL Connections

Martin supports PostgreSQL `sslmode` settings: `disable`, `prefer`, `require`, `verify-ca` and `verify-full`.
See the [PostgreSQL docs](https://www.postgresql.org/docs/current/libpq-ssl.html) for mode descriptions.
Certificates can be provided in the configuration file or via environment variables (same as `psql`).
Environment variables apply to all PostgreSQL connections.
See [environment vars](env-vars.md) for details.

By default, `sslmode` is `prefer` - SSL is used if the server supports it, but the connection proceeds without SSL if not supported.
This matches `psql` default behavior. Use the `sslmode` parameter to specify a different mode:

```bash
martin postgresql://user:password@host/db?sslmode=verify-full
```

For a practical walkthrough of SSL certificate setup — including creation, configuration, and troubleshooting — see our [PostgreSQL SSL Certificates Recipe](pg-ssl-certificates.md).
