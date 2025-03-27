## Recipes

### Using with DigitalOcean PostgreSQL

You can use Martin
with [Managed PostgreSQL from DigitalOcean](https://www.digitalocean.com/products/managed-databases-postgresql/) with
PostGIS extension

First, you need to download the CA certificate and get your cluster connection string from
the [dashboard](https://cloud.digitalocean.com/databases). After that, you can use the connection string and the CA
certificate to connect to the database

```bash
martin --ca-root-file ./ca-certificate.crt \
       postgresql://user:password@host:port/db?sslmode=require
```

### Using with Heroku PostgreSQL

You can use Martin with [Managed PostgreSQL from Heroku](https://www.heroku.com/postgres) with PostGIS extension

```bash
heroku pg:psql -a APP_NAME -c 'create extension postgis'
```

Use the same environment variables as
Heroku [suggests for psql](https://devcenter.heroku.com/articles/heroku-postgres-via-mtls#step-2-configure-environment-variables).

```bash
export DATABASE_URL=$(heroku config:get DATABASE_URL -a APP_NAME)
export PGSSLCERT=DIRECTORY/PREFIXpostgresql.crt
export PGSSLKEY=DIRECTORY/PREFIXpostgresql.key
export PGSSLROOTCERT=DIRECTORY/PREFIXroot.crt

martin
```

You may also be able to validate SSL certificate with an explicit sslmode, e.g.

```bash
export DATABASE_URL="$(heroku config:get DATABASE_URL -a APP_NAME)?sslmode=verify-ca"
```
