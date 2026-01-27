# Martin E2E Tests

End-to-end tests that spawn actual Martin binaries and make real HTTP requests.

## Running Tests

E2E tests are marked with `#[ignore]` to separate them from unit tests.

```bash
# Using justfile
just test-e2e

# Specific test file
just test-e2e tile_serving
```

## Prerequisites

- **Binaries**: Built automatically on first run, or `cargo build --workspace --bins`
- **Fixtures**: Located in `../tests/fixtures/`
- **PostgreSQL**: Set `DATABASE_URL` or use `just start` for database tests
- **Optional tools**: `sqlite3`, `gdal` for some validation tests
