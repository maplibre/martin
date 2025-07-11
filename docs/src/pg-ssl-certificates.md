# PostgreSQL SSL Certificates

Martin supports SSL certificate authentication for PostgreSQL connections. This guide covers certificate generation, PostgreSQL configuration, and Martin setup.

## When to Use SSL Certificates

Use SSL certificates for:

- Deployments where martin and Postgis are on separate machines
- Compliance requirements (PCI DSS, HIPAA, etc.)
- Cloud PostgreSQL deployments

## SSL Modes

PostgreSQL supports several SSL modes:

| Mode          | Description                                          |
|---------------|------------------------------------------------------|
| `disable`     | No SSL connection                                    |
| `prefer`      | Try SSL first, fall back to non-SSL (default)        |
| `require`     | Require SSL, don't verify certificate                |
| `verify-ca`   | Require SSL and verify server certificate against CA |
| `verify-full` | Require SSL, verify certificate and hostname         |

`verify-ca` verifies the server certificate is signed by a trusted CA but doesn't check hostname matching. `verify-full` provides maximum security by verifying both CA signature and hostname matching.

## Generating Certificates

### Self-Signed Certificates

For development and testing:

```bash
# Create certificate directory
mkdir -p ~/certs && cd ~/certs

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
version: '3.8'
services:
  postgres:
    image: postgis/postgis:16-3.5
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

## Cloud Providers

### AWS RDS

```bash
# Download RDS CA certificate
curl -o ~/certs/rds-ca-2019-root.pem https://s3.amazonaws.com/rds-downloads/rds-ca-2019-root.pem

# Configure Martin
export PGSSLROOTCERT=~/certs/rds-ca-2019-root.pem
export DATABASE_URL="postgresql://username:password@rds-endpoint:5432/dbname?sslmode=verify-full"
martin
```

### Google Cloud SQL

```bash
# Download CA certificate from console
export PGSSLROOTCERT=~/certs/server-ca.pem
export PGSSLCERT=~/certs/client-cert.pem
export PGSSLKEY=~/certs/client-key.pem
export DATABASE_URL="postgresql://username:password@google-cloud-ip:5432/dbname?sslmode=verify-full"
martin
```

### Azure Database for PostgreSQL

```bash
# Download Azure CA certificate
curl -o ~/certs/azure-ca.pem https://www.digicert.com/CACerts/BaltimoreCyberTrustRoot.crt.pem

# Configure Martin
export PGSSLROOTCERT=~/certs/azure-ca.pem
export DATABASE_URL="postgresql://username:password@azure-server:5432/dbname?sslmode=verify-full"
martin
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
