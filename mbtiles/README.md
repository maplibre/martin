# Development

```bash
# To update sqlx files, first install sqlx-cli:
cargo install sqlx-cli --no-default-features --features sqlite,native-tls

# Prepare DB schema (from the ./mbtiles dir)
cargo sqlx prepare --database-url sqlite://$PWD/fixtures/geography-class-jpg.mbtiles
```
