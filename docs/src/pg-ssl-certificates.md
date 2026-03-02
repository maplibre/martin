# PostgreSQL SSL Certificates

Martin supports SSL certificate authentication for PostgreSQL connections. This guide covers certificate generation, PostgreSQL configuration, and Martin setup.

## When to Use SSL Certificates

Use SSL certificates for:

- Deployments where martin and Postgis are on separate machines
- Compliance requirements (PCI DSS, HIPAA, etc.)
- Cloud PostgreSQL deployments
- High-security environments requiring certificate-based authentication

## SSL Modes

| sslmode       | Eaves-<br/>dropping<br/>protection | MITM <br/>protection       | Statement                                                                                                                                   |
|---------------|------------------------------------|----------------------------|---------------------------------------------------------------------------------------------------------------------------------------------|
| `disable`     | ⛔                                 | ⛔                         | I don't care about security, and I don't want to pay the overhead of encryption.                                                            |
| `allow`       | 🤷                                 | ⛔                         | I don't care about security, but I will pay the overhead of encryption if the server insists on it.                                         |
| `prefer`      | 🤷                                 | ⛔                         | I don't care about encryption, but I wish to pay the overhead of encryption if the server supports it.                                      |
| `require`     | ✅                                 | ⛔                         | I want my data to be encrypted, and I accept the overhead. I trust that the network will make sure I always connect to the server I want.   |
| `verify-ca`   | ✅                                 | Depends <br/> on CA policy | I want my data encrypted, and I accept the overhead. I want to be sure that I connect to a server that I trust.                             |
| `verify-full` | ✅                                 | ✅                         | I want my data encrypted, and I accept the overhead. I want to be sure that I connect to a server I trust, and that it's the one I specify. |

Our recommendation: **`verify-full` or `allow`**.
There are not many cases where anything in between makes sense.

In particular, the default mode (`prefer`) does not make much sense.
From the postgres documentation:

> As is shown in the table, this makes no sense from a security point of view, and it only promises performance overhead if possible.
> It is only provided as the default for backward compatibility, and is not recommended in secure deployments.

For a fuller explanation of the different tradeoffs, refer to the [PostgreSQL SSL Certificates documentation](https://www.postgresql.org/docs/current/libpq-ssl.html#LIBPQ-SSL-CONFIG).

## Generating Certificates

For basic SSL encryption, you need:

- `server-cert.pem` - PostgreSQL server certificate
- `server-key.pem` - PostgreSQL server private key
- `ca-cert.pem` - Certificate Authority certificate

```raw
┌─────────────────┐    SSL/TLS     ┌─────────────────┐
│     Martin      │◄─────────────►│   PostgreSQL    │
└─────────────────┘   verify-full  └─────────────────┘
         │                                   │
    ┌─────────┐                        ┌─────────────┐
    │ CA Cert │                        │ Server Cert │
    │         │                        │ Server Key  │
    └─────────┘                        └─────────────┘
```

### Self-Signed Certificates

To generate certificates as a CA, you will need a private key.
To verify the certificate, you will need the CA certificate.

```bash
# Generate CA private key
openssl genrsa -out ca-key.pem 3072

# Generate CA certificate
openssl req -new -x509 -days 365 -key ca-key.pem -out ca-cert.pem \
    -subj "/C=US/ST=State/L=City/O=Organization/CN=Test CA"
```

You can then generate a server certificates:

```bash
# Generate server private key
openssl genrsa -out server-key.pem 3072

# Generate server certificate signing request with SAN extension
openssl req -new -key server-key.pem -out server-csr.pem \
    -subj "/C=US/ST=State/L=City/O=Organization/CN=localhost" \
    -addext "subjectAltName = DNS:localhost"

# Generate server certificate signed by CA with SAN extension
openssl x509 -req -days 365 -in server-csr.pem \
    -CA ca-cert.pem -CAkey ca-key.pem \
    -CAcreateserial -out server-cert.pem -extensions v3_req \
    -extfile <(printf "[v3_req]\nsubjectAltName = DNS:localhost")

# Set permissions
chmod 400 *-key.pem
chmod 444 *-cert.pem ca-cert.pem
```

### Production Certificates

For production, use certificates from:

- Regular Certificate Authorities (Let's Encrypt, DigiCert, GlobalSign)
- Cloud provider managed Certificate Authorities
- Organization-Internal Certificate Authority

## PostgreSQL Configuration

```yaml
services:
  db:
    image: postgis/postgis:17-3.5
    environment:
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: password
    ports:
      - "5432:5432"
    volumes:
      - ./server-cert.pem:/var/lib/postgresql/server.crt:ro
      - ./server-key.pem:/var/lib/postgresql/server.key:ro
    command:
      - -c
      - ssl=on
      - -c
      - ssl_cert_file=/var/lib/postgresql/server.crt
      - -c
      - ssl_key_file=/var/lib/postgresql/server.key
```

```bash
docker compose up
```

!!! tip
    > Postgres requires specific file permissions and ownership for SSL certificates.
    > In docker this can be a bit tricky:
    >
    > alpine images have `70:70` as the default `user:group`
    > debian images have `999:999` as the default `user:group`
    >
    > You can change this by running the following commands:
    >
    > ```bash
    > chown 999:999 *.pem
    > chmod 400 *.pem
    >```

## Testing with psql

Test SSL Connection via

```bash
export PGSSLROOTCERT=ca-cert.pem
psql "postgres://postgres:password@localhost:5432/postgres?sslmode=verify-full"
```

!!! tip
    > If you get file permission errors, make sure the current user can access the files.
    > The previous step may set them to not readable by the current user.

Then, verify SSL Status by

```sql
-- Enable SSL info extension (required for ssl_is_used function)
CREATE EXTENSION IF NOT EXISTS sslinfo;

-- Check SSL status
SELECT ssl_is_used();

-- SSL connection details
SELECT * FROM pg_stat_ssl WHERE pid = pg_backend_pid();
```

## Martin Configuration

Martin can be configured using environment variables, the CLI, or the configuration file.
Which of them you choose is up to you.
You do not need to configure things twice.

??? "Environment Variables (click to expand)"
    ```bash
    export PGSSLROOTCERT=./ca-cert.pem
    export DATABASE_URL="postgres://postgres:password@localhost:5432/postgres?sslmode=verify-full"
    martin
    ```

??? "Configuration File (click to expand)"
    ```yaml
    postgres:
      ssl_root_cert: './ca-cert.pem'
      connection_string: 'postgres://postgres:password@localhost:5432/postgres?sslmode=verify-full'
    ```

??? "Command Line (click to expand)"
    ```bash
    martin --ca-root-file ./ca-cert.pem \
          "postgres://postgres:password@localhost:5432/postgres?sslmode=verify-full"
    ```

## Troubleshooting

You can get more context via the following commands:

```bash
export PGSSLMODE=verify-full
export PGSSLROOTCERT=./ca-cert.pem
# Verbose psql
psql -h localhost -U postgres -d postgres -v

# Debug Martin
RUST_LOG=debug RUST_LOG_FORMAT=pretty martin postgres://...
```

These are the errors that can occur:

??? "Certificate verification failed (click to expand)"
    - Check server certificate is signed by the CA
    - Verify CA certificate path in `PGSSLROOTCERT`
    - Ensure certificate files are readable

??? "Hostname verification failed (click to expand)"
    - Server certificate CN/SAN must match hostname
    - Use `verify-ca` instead of `verify-full` if hostname doesn't match

??? "Permission denied (click to expand)"
    - Check certificate file permissions
    - Private keys should be `chmod 400` and readable by the user running the application

??? "Connection refused (click to expand)"
    - Verify PostgreSQL accepts SSL connections
    - Check `pg_hba.conf` allows SSL from your IP

## Security Best Practices if using postgres via SSL

- Use at least 3072-bit RSA keys
- Protect private keys with restricted permissions (`chmod 400`)
- Rotate certificates before expiration
- Use `verify-full` in production
- Monitor certificate expiration
- Store `ca-key.pem` securely (only needed for certificate management)
- Use secure secret management for production certificates
