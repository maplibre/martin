# Martin Tile Server Development Guide

Martin is a blazing fast and lightweight tile server with PostGIS, MBTiles, and PMTiles support written in Rust. 

**ALWAYS follow these instructions first and only fallback to additional search or context gathering if the information here is incomplete or found to be in error.**

## Working Effectively

### Essential Setup
Bootstrap the development environment in this exact order:

```bash
# Install just command runner
cargo install just --locked

# Validate all required tools are present
just validate-tools

# Start test database
just start

# Build the project - NEVER CANCEL: Full build takes 3-4 minutes.
cargo build --workspace

# 5Install frontend dependencies
cd martin/martin-ui && npm install && cd ../..

### Build Commands and Timing
**CRITICAL**: NEVER CANCEL builds or tests before completion. Set appropriate timeouts:

```bash
# Build check - takes ~6 minutes. NEVER CANCEL. Set timeout to 15+ minutes.
just check

# Full build - takes ~3 minutes. NEVER CANCEL. Set timeout to 10+ minutes.
cargo build --workspace

# Release build - takes ~5 minutes. NEVER CANCEL. Set timeout to 15+ minutes.
cargo build --workspace --release
```

### Test Commands and Timing
**CRITICAL**: Tests may take 10-15+ minutes total. NEVER CANCEL. Set timeout to 30+ minutes.

```bash
# Unit tests - takes ~2 minutes total. NEVER CANCEL. Set timeout to 5+ minutes.
just test-cargo --all-targets

# Frontend tests - takes ~11 seconds. Set timeout to 2+ minutes.
just test-frontend

# Integration tests - requires database. Set timeout to 10+ minutes.
# Note: Some integration tests may fail due to S3/network issues - this is expected in CI environments
export DATABASE_URL='postgres://postgres:postgres@localhost:5411/db'
tests/test.sh

# Run all tests - takes 10-15 minutes. NEVER CANCEL. Set timeout to 30+ minutes.
just test
```

### Development Server
```bash
# Start development server with web UI
just run --webui enable-for-all

# Server will be available at:
# - API: http://localhost:3000/catalog  
# - Web UI: http://localhost:3000/
# - Health check: http://localhost:3000/health
```

### Code Quality Commands
```bash
# Format code - takes ~10 seconds
just fmt

# Lint code - takes ~2 minutes. Set timeout to 5+ minutes.
just clippy

# Type check frontend - takes ~5 seconds
just type-check

# All linting together - takes ~2 minutes
just lint
```

### Database Management
```bash
# Start test database (PostgreSQL 16 with PostGIS)
just start

# Stop database
just stop

# Restart database 
just restart

# Connect to database
just psql

# Print connection string
just print-conn-str
```

## Validation Requirements

### Manual Validation Scenarios
**ALWAYS** run through these complete scenarios after making changes:

#### Scenario 1: Basic MBTiles Server
```bash
# 1. Start with clean build
cargo build --workspace
just start

# 2. Start server with MBTiles files
cargo run --bin martin -- --webui enable-for-all tests/fixtures/mbtiles

# 3. Validate server responds
curl -s http://localhost:3000/catalog | head -10
curl -s http://localhost:3000/health

# 4. Test a tile endpoint  
curl -s http://localhost:3000/world_cities/0/0/0 | head -1

# 5. Access web UI in browser (if possible)
# Navigate to http://localhost:3000/
```

#### Scenario 2: PostgreSQL Database Integration  
```bash
# 1. Ensure database is running and initialized
just start
PGHOST=localhost PGPORT=5411 PGUSER=postgres PGPASSWORD=postgres PGDATABASE=db tests/fixtures/initdb.sh

# 2. Start server with database connection
export DATABASE_URL='postgres://postgres:postgres@localhost:5411/db'
cargo run --bin martin -- --webui enable-for-all

# 3. Validate database tables are detected
curl -s http://localhost:3000/catalog | grep -i table
```

#### Scenario 3: CLI Tools Validation
```bash
# Test martin-cp tool
cargo run --bin martin-cp -- --help

# Test mbtiles tool  
cargo run --bin mbtiles -- --help

# Test actual functionality
cargo run --bin mbtiles -- --help meta-all tests/fixtures/mbtiles/world_cities.mbtiles
```

### CI Validation Commands
**ALWAYS** run these before completing any changes:
```bash
# Complete CI validation - takes 15-20 minutes. NEVER CANCEL. Set timeout to 45+ minutes.
just ci-test

# Individual validation steps:
just test-fmt     # Format check - 10 seconds
just clippy       # Linting - 2 minutes. Set timeout to 5+ minutes.
just check-doc    # Documentation - 1 minute
just test         # All tests - 15 minutes
```

## Critical Timing and Timeout Information

### Build Operations
- **Build check (`just check`)**: 6 minutes - Set timeout to 15+ minutes
- **Full build (`cargo build`)**: 3-4 minutes - Set timeout to 10+ minutes  
- **Release build**: 5+ minutes - Set timeout to 15+ minutes
- **Frontend build**: 30 seconds - Set timeout to 2+ minutes

### Test Operations  
- **Unit tests (`just test-cargo`)**: 2 minutes - Set timeout to 5+ minutes
- **Frontend tests**: 11 seconds - Set timeout to 2+ minutes
- **Integration tests**: Variable, 5-10 minutes - Set timeout to 20+ minutes
- **Complete test suite (`just test`)**: 10-15 minutes - Set timeout to 30+ minutes
- **CI test suite (`just ci-test`)**: 15-20 minutes - Set timeout to 45+ minutes

### Database Operations
- **Database startup (`just start`)**: 15 seconds - Set timeout to 2+ minutes
- **Database initialization**: 30 seconds - Set timeout to 2+ minutes

## Common Issues and Troubleshooting

### Build Issues
- If `just` command not found: `cargo install just --locked`
- If build fails with missing tools: `just validate-tools` then install missing dependencies
- If PostgreSQL connection fails: Check `just start` output and use `just restart`
- If frontend tests fail: Run `cd martin/martin-ui && npm install` first

### Integration Test Issues  
- Some tests may fail due to S3/AWS configuration in CI environments - this is expected
- Database connection issues: Ensure `DATABASE_URL='postgres://postgres:postgres@localhost:5411/db'`
- PMTiles HTTP tests: Ensure nginx fileserver is running via `just start`

### Performance Notes
- Martin is optimized for speed and heavy traffic
- Release builds are significantly faster than debug builds for performance testing  
- Database connection pooling is configured for high throughput
- Caching is enabled by default (512MB)

## Key Project Structure

### Main Components
- `martin/` - Main martin server crate  
- `martin-core/` - Core shared functionality
- `martin-tile-utils/` - Tile manipulation utilities
- `mbtiles/` - MBTiles format support and CLI tool
- `martin/martin-ui/` - React-based web UI
- `tests/` - Integration tests and fixtures
- `justfile` - Main task runner configuration

### Important Configuration Files
- `Cargo.toml` - Workspace configuration
- `justfile` - Development task definitions  
- `docker-compose.yml` - Test database and services
- `martin/martin-ui/package.json` - Frontend dependencies
- `.github/workflows/ci.yml` - CI pipeline configuration

### Test Data Locations
- `tests/fixtures/mbtiles/` - Sample MBTiles files
- `tests/fixtures/pmtiles/` - Sample PMTiles files
- `tests/fixtures/cog/` - Cloud Optimized GeoTIFF files
- `tests/fixtures/sprites/` - SVG sprite sources
- `tests/fixtures/fonts/` - Font files
- `tests/fixtures/styles/` - MapLibre style files

## Quick Reference Commands
```bash
# Development workflow
just help         # Show common commands
just --list       # Show all available commands  
just validate-tools  # Check required tools
just start        # Start test database
just run          # Start martin server
just test         # Run all tests (15+ minutes)
just fmt          # Format code
just clippy       # Lint code  
just book         # Build documentation
just stop         # Stop test database

# Build variants
just check        # Quick build check (6+ minutes)
cargo build       # Debug build (3+ minutes)  
cargo build --release  # Release build (5+ minutes)

# Testing variants  
just test-cargo   # Unit tests only (2+ minutes)
just test-frontend # Frontend tests (11 seconds)
just test-int     # Integration tests (variable)
just ci-test      # Full CI validation (20+ minutes)
```

Remember: Martin is a production-ready tile server handling heavy geographic workloads. Always validate changes with realistic data scenarios and never cancel long-running builds or tests.