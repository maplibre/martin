# PostgreSQL SSL Certificates

Martin supports SSL certificate authentication for PostgreSQL connections. This guide covers certificate generation, PostgreSQL configuration, and Martin setup.

## When to Use SSL Certificates

Use SSL certificates for:

- Deployments where martin and Postgis are on separate machines
- Compliance requirements (PCI DSS, HIPAA, etc.)
- Cloud PostgreSQL deployments

## SSL Modes

| sslmode       | Eaves-<br/>dropping<br/>protection | MITM <br/>protection      | Statement                                                                                                                                   |
|---------------|--------------------------|----------------------|---------------------------------------------------------------------------------------------------------------------------------------------|
| `disable`     | ⛔                        | ⛔                    | I don't care about security, and I don't want to pay the overhead of encryption.                                                            |
| `allow`       | 🤷                        | ⛔                    | I don't care about security, but I will pay the overhead of encryption if the server insists on it.                                         |
| `prefer`      | 🤷                        | ⛔                    | I don't care about encryption, but I wish to pay the overhead of encryption if the server supports it.                                      |
| `require`     | ✅                        | ⛔                    | I want my data to be encrypted, and I accept the overhead. I trust that the network will make sure I always connect to the server I want.   |
| `verify-ca`   | ✅                        | Depends <br/> on CA policy | I want my data encrypted, and I accept the overhead. I want to be sure that I connect to a server that I trust.                             |
| `verify-full` | ✅                        | ✅                    | I want my data encrypted, and I accept the overhead. I want to be sure that I connect to a server I trust, and that it's the one I specify. |

Our recommendation: **`verify-full` or `allow`**.
There are not many cases where anything in between makes sense.

In particular, the default mode (`prefer`) does not make much sense.
From the postgres documentation:

> As is shown in the table, this makes no sense from a security point of view, and it only promises performance overhead if possible.
> It is only provided as the default for backward compatibility, and is not recommended in secure deployments.

For a fuller explanation of the different tradeoffs, refer to the [PostgreSQL SSL Certificates documentation](https://www.postgresql.org/docs/current/libpq-ssl.html#LIBPQ-SSL-CONFIG).

## Generating Certificates

### Self-Signed Certificates

For development and testing:

```bash
# Create certificate directory
mkdir -p certs && cd certs

# Generate CA private key
openssl genrsa -out ca-key.pem 4096

# Generate CA certificate
openssl req -new -x509 -days 365 -key ca-key.pem -out ca-cert.pem \
    -subj "/C=US/ST=State/L=City/O=Organization/CN=Test CA"

# Generate server private key
openssl genrsa -out server-key.pem 4096

# Generate server certificate signing request
openssl req -new -key server-key.pem -out server-csr.pem \
    -subj "/C=US/ST=State/L=City/O=Organization/CN=localhost"

# Generate server certificate signed by CA
openssl x509 -req -days 365 -in server-csr.pem -CA ca-cert.pem -CAkey ca-key.pem \
    -CAcreateserial -out server-cert.pem

# Generate client private key
openssl genrsa -out client-key.pem 4096

# Generate client certificate signing request
openssl req -new -key client-key.pem -out client-csr.pem \
    -subj "/C=US/ST=State/L=City/O=Organization/CN=postgres"

# Generate client certificate signed by CA
openssl x509 -req -days 365 -in client-csr.pem -CA ca-cert.pem -CAkey ca-key.pem \
    -CAcreateserial -out client-cert.pem

# Set permissions
chmod 400 *-key.pem
chmod 444 *-cert.pem ca-cert.pem

# Exit certificate directory
cd ..
```

### Production Certificates

For production, use certificates from:

- Certificate Authorities (Let's Encrypt, DigiCert, GlobalSign)
- Cloud provider managed certificates
- Internal organizational CA

## PostgreSQL Configuration

### Server Configuration

Edit `postgresql.conf`:

```conf
# Enable SSL
ssl = on

# Certificate files
ssl_cert_file = '/path/to/server-cert.pem'
ssl_key_file = '/path/to/server-key.pem'
ssl_ca_file = '/path/to/ca-cert.pem'

# SSL cipher configuration (optional)
ssl_ciphers = 'HIGH:MEDIUM:+3DES:!aNULL'
ssl_prefer_server_ciphers = on
```

### Client Authentication

Edit `pg_hba.conf`:

```conf
# TYPE  DATABASE        USER            ADDRESS                 METHOD

# SSL connections only
hostssl all             all             0.0.0.0/0               md5

# SSL with client certificates
hostssl all             all             0.0.0.0/0               cert
```

### Docker Configuration

```yaml
services:
  postgres:
    image: postgis/postgis:17-3.5
    command: |
      postgres
      -c ssl=on
      -c ssl_cert_file=/etc/ssl/certs/server-cert.pem
      -c ssl_key_file=/etc/ssl/private/server-key.pem
      -c ssl_ca_file=/etc/ssl/certs/ca-cert.pem
    volumes:
      - ./certs/server-cert.pem:/etc/ssl/certs/server-cert.pem:ro
      - ./certs/server-key.pem:/etc/ssl/private/server-key.pem:ro
      - ./certs/ca-cert.pem:/etc/ssl/certs/ca-cert.pem:ro
    environment:
      - POSTGRES_DB=mydb
      - POSTGRES_USER=postgres
      - POSTGRES_PASSWORD=password
    ports:
      - 5432:5432
```

## Testing with psql

### Test SSL Connection

```bash
# Basic SSL connection
psql "postgresql://postgres:password@localhost:5432/mydb?sslmode=require"

# Certificate verification
psql "postgresql://postgres:password@localhost:5432/mydb?sslmode=verify-ca" \
     --set=PGSSLROOTCERT=~/certs/ca-cert.pem

# Full verification
psql "postgresql://postgres:password@localhost:5432/mydb?sslmode=verify-full" \
     --set=PGSSLROOTCERT=~/certs/ca-cert.pem

# Client certificate authentication
psql "postgresql://postgres@localhost:5432/mydb?sslmode=verify-full" \
     --set=PGSSLROOTCERT=~/certs/ca-cert.pem \
     --set=PGSSLCERT=~/certs/client-cert.pem \
     --set=PGSSLKEY=~/certs/client-key.pem
```

### Verify SSL Status

```sql
-- Check SSL status
SELECT ssl_is_used();

-- SSL connection details
SELECT * FROM pg_stat_ssl WHERE pid = pg_backend_pid();
```

## Martin Configuration

### Environment Variables

Configure Martin for SSL using environment variables:

```bash
# Root CA certificate
export PGSSLROOTCERT=~/certs/ca-cert.pem

# Client certificate (if required)
export PGSSLCERT=~/certs/client-cert.pem
export PGSSLKEY=~/certs/client-key.pem

# Database connection
export DATABASE_URL="postgresql://postgres:password@localhost:5432/mydb?sslmode=verify-full"

martin
```

### Configuration File

```yaml
postgres:
  connection_string: 'postgresql://postgres:password@localhost:5432/mydb?sslmode=verify-full'

  # SSL certificate files
  ssl_root_cert: '~/certs/ca-cert.pem'
  ssl_cert: '~/certs/client-cert.pem'
  ssl_key: '~/certs/client-key.pem'
```

### Command Line

```bash
martin --ca-root-file ~/certs/ca-cert.pem \
       "postgresql://postgres:password@localhost:5432/mydb?sslmode=verify-full"
```

## Troubleshooting

You can get more context via the following commands:

```bash
# Verbose psql
PGSSLMODE=verify-full PGSSLROOTCERT=~/certs/ca-cert.pem psql -h localhost -U postgres -d mydb -v

# Debug Martin
RUST_LOG=debug martin postgresql://...
```

- <details>
  <summary>Certificate verification failed (click to expand)</summary>

  - Check server certificate is signed by the CA
  - Verify CA certificate path in `PGSSLROOTCERT`
  - Ensure certificate files are readable

  </details>
- <details>
  <summary>Hostname verification failed (click to expand)</summary>

  - Server certificate CN/SAN must match hostname
  - Use `verify-ca` instead of `verify-full` if hostname doesn't match

  </details>
- <details>
  <summary>Permission denied (click to expand)</summary>

  - Check certificate file permissions
  - Private keys should be `chmod 400`

  </details>
- <details>
  <summary>Connection refused (click to expand)</summary>

  - Verify PostgreSQL accepts SSL connections
  - Check `pg_hba.conf` allows SSL from your IP

  </details>

## Security Best Practices

- Use at least 2048-bit RSA keys
- Protect private keys with restricted permissions
- Rotate certificates before expiration
- Use `verify-full` in production when possible
- Monitor certificate expiration
- Use secure secret management for production certificates
