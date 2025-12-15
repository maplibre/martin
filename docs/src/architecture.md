# Architecture

This document provides a high-level overview of Martin's architecture, describing its major components, how they interact, and the rationale behind key design decisions.

## Overview

Martin is a blazing fast tile server written in Rust that generates and serves vector tiles on the fly from multiple data sources. It is designed to handle heavy traffic and optimize for speed while maintaining a clean separation of concerns.

```mermaid
graph TB
    Client[Map Client<br/>MapLibre, Leaflet, etc.]
    
    subgraph Martin["Martin Tile Server"]
        CLI[CLI Entry Point<br/>martin binary]
        Server[HTTP Server<br/>Actix-Web]
        
        subgraph Sources["Tile Sources"]
            PG[PostgreSQL<br/>Tables & Functions]
            MBT[MBTiles Files]
            PMT[PMTiles Files<br/>Local & Remote]
            COG[Cloud Optimized<br/>GeoTIFF]
        end
        
        subgraph Resources["Supporting Resources"]
            Sprites[Sprite Generation<br/>SVG to PNG]
            Fonts[Font Glyphs<br/>PBF Format]
            Styles[MapLibre Styles<br/>JSON]
        end
        
        Catalog[Tile Catalog<br/>Source Registry]
        Cache[Tile/Resources Cache<br/>Moka]
    end
    
    subgraph Storage["Data Storage"]
        DB[(PostgreSQL<br/>PostGIS)]
        Files[File System<br/>MBTiles/PMTiles]
        S3[Object Storage<br/>S3/Azure/GCP]
    end
    
    Client -->|HTTP Requests| Server
    Server --> Catalog
    Catalog --> Sources
    Server --> Resources
    Server --> Cache
    
    PG --> DB
    MBT --> Files
    PMT --> Files
    PMT --> S3
    COG --> Files
    COG --> S3
    
    Cache -.->|Cached Tiles/Resources| Client
```

## Core Components

Martin's architecture is organized into four main Rust crates, each with distinct responsibilities:

### 1. martin (Main Server)

**Purpose**: The main tile server binary and HTTP service layer.

**Location**: `/martin`

**Key Responsibilities**:
- HTTP server using Actix-Web framework
- Request routing and endpoint handling
- Configuration parsing (CLI args, env vars, config files)
- Tile source discovery and initialization
- Serving the Web UI for tile inspection

**Main Modules**:
- `src/bin/martin.rs` - Server entry point
- `src/bin/martin-cp.rs` - Bulk tile copying tool
- `src/srv/` - HTTP service handlers
  - `server.rs` - Main server setup and routing
  - `tiles/` - Tile serving endpoints
  - `fonts.rs` - Font glyph serving
  - `sprites.rs` - Sprite serving
  - `styles.rs` - Style serving
- `src/config/` - Configuration management
  - `args/` - CLI argument parsing
  - `file/` - Config file parsing
- `martin-ui/` - React-based web interface

### 2. martin-core (Shared Library)

**Purpose**: Core abstractions and implementations for tile sources and supporting resources.

**Location**: `/martin-core`

**Key Responsibilities**:
- Abstract tile source traits and implementations
- PostgreSQL connection pooling and query execution
- MBTiles and PMTiles reading
- Cloud Optimized GeoTIFF (COG) tile extraction
- Sprite, font, and style resource generation
- Tile format handling (MVT protocol buffers)

**Main Modules**:
- `src/tiles/` - Tile source implementations
  - `postgres/` - PostGIS table and function sources
  - `mbtiles/` - MBTiles file source
  - `pmtiles/` - PMTiles file source
  - `cog/` - Cloud Optimized GeoTIFF source
  - `catalog.rs` - Source registry and management
- `src/resources/` - Supporting resources
  - `sprites/` - SVG sprite generation
  - `fonts/` - Font glyph generation
  - `styles/` - MapLibre style handling

### 3. mbtiles (MBTiles Library & CLI)

**Purpose**: MBTiles format support and manipulation tools.

**Location**: `/mbtiles`

**Key Responsibilities**:
- SQLite-based MBTiles reading and writing
- Metadata management
- Tile compression (gzip, brotli)
- Validation and integrity checking
- Diff/patch operations between MBTiles files
- Schema management

**Main Modules**:
- `src/lib.rs` - Core MBTiles library
- `src/bin/mbtiles.rs` - CLI tool
- `sql/` - SQL schema and migrations

### 4. martin-tile-utils (Tile Utilities)

**Purpose**: Low-level tile manipulation and conversion utilities.

**Location**: `/martin-tile-utils`

**Key Responsibilities**:
- Tile coordinate system conversions
- Tile encoding/decoding
- Tile format utilities
- Bounding box calculations

## Data Flow

### Tile Request Flow

```mermaid
sequenceDiagram
    participant Client
    participant Server as HTTP Server
    participant Catalog
    participant Cache
    participant Source as Tile Source
    participant DB as Data Store

    Client->>Server: GET /source_id/z/x/y
    Server->>Catalog: Resolve source_id
    Catalog-->>Server: Source reference
    Server->>Cache: Check cache
    
    alt Tile in cache
        Cache-->>Server: Cached tile
        Server-->>Client: 200 OK (tile data)
    else Tile not in cache
        Server->>Source: Get tile(z, x, y)
        Source->>DB: Query data
        DB-->>Source: Raw data
        Source->>Source: Generate MVT
        Source-->>Server: Tile data
        Server->>Cache: Store tile
        Server-->>Client: 200 OK (tile data)
    end
```

### Configuration and Initialization Flow

```mermaid
sequenceDiagram
    participant CLI
    participant Config as Config Parser
    participant Discovery as Source Discovery
    participant Sources as Tile Sources
    participant Server as HTTP Server

    CLI->>Config: Parse args & config file
    Config->>Discovery: Initialize sources
    
    alt PostgreSQL Source
        Discovery->>Discovery: Connect to database
        Discovery->>Discovery: Query tables & functions
        Discovery->>Sources: Register table sources
        Discovery->>Sources: Register function sources
    end
    
    alt File Sources
        Discovery->>Discovery: Scan MBTiles files
        Discovery->>Discovery: Scan PMTiles files
        Discovery->>Sources: Register file sources
    end
    
    Sources-->>Server: Source catalog
    Server->>Server: Setup routes
    Server->>Server: Start HTTP listener
```

### Resource Generation Flow

```mermaid
sequenceDiagram
    participant Client
    participant Server
    participant Sprite as Sprite Generator
    participant Font as Font Generator
    participant FS as File System

    Client->>Server: GET /sprite/sprite_id
    Server->>Sprite: Generate sprite
    Sprite->>FS: Read SVG files
    FS-->>Sprite: SVG data
    Sprite->>Sprite: Render to PNG
    Sprite->>Sprite: Generate JSON index
    Sprite-->>Server: Sprite sheet
    Server-->>Client: PNG/JSON response

    Client->>Server: GET /font/fontstack/range
    Server->>Font: Generate glyphs
    Font->>FS: Read font files
    FS-->>Font: Font data
    Font->>Font: Rasterize glyphs
    Font->>Font: Encode as PBF
    Font-->>Server: Glyph data
    Server-->>Client: PBF response
```

## Key Design Decisions

### 1. Rust for Performance and Safety

**Rationale**: Martin is written in Rust to achieve:
- **Performance**: Near-C performance for CPU-intensive tile generation
- **Memory Safety**: No null pointer dereferences or buffer overflows
- **Concurrency**: Safe concurrent access without data races
- **Zero-cost Abstractions**: High-level code with low-level performance

### 2. Actix-Web Framework

**Rationale**: Actix-Web provides:
- High-performance async HTTP server
- Mature middleware ecosystem
- Excellent streaming support for large tiles
- Built-in compression and caching headers
- Easy integration with Prometheus metrics

### 3. Async/Await Throughout

**Rationale**: Async I/O enables:
- Handling thousands of concurrent connections
- Non-blocking database queries
- Efficient file I/O operations
- Minimal thread overhead

### 4. Crate Separation

**Rationale**: Splitting into multiple crates provides:
- **martin-core**: Reusable as a library in other projects
- **mbtiles**: Standalone MBTiles tooling
- **martin**: Server-specific concerns (HTTP, config)
- **martin-tile-utils**: Shared low-level utilities

This enables:
- Using Martin as a library (embedding in other Rust projects)
- Using MBTiles tools independently
- Clear API boundaries and versioning

### 5. PostgreSQL Connection Pooling

**Rationale**: Using `deadpool-postgres`:
- Maintains a pool of persistent database connections
- Avoids connection overhead per request
- Configurable pool size for scaling
- Automatic connection health checking

### 6. In-Memory Tile Caching

**Rationale**: Using `moka` cache provides:
- Fast LRU cache with TTL support
- Automatic eviction of least-used tiles
- Configurable memory limits (default 512MB)
- Thread-safe concurrent access
- Significant performance improvement for repeated tile requests

### 7. Automatic Source Discovery

**Rationale**: Martin automatically discovers:
- PostgreSQL tables with geometry columns
- PostgreSQL functions returning MVT
- MBTiles/PMTiles files in directories

This reduces configuration burden and enables zero-config operation for common scenarios.

### 8. Multi-Protocol Tile Support

**Rationale**: Supporting multiple source types enables:
- **PostgreSQL**: Dynamic tiles from live data
- **MBTiles**: Pre-generated tile archives
- **PMTiles**: Cloud-native single-file archives
- **COG**: Direct serving from GeoTIFF files

This flexibility allows operators to choose the best storage format for their use case.

### 9. On-the-Fly Resource Generation

**Rationale**: Generating sprites, fonts, and styles dynamically:
- Eliminates need for pre-processing
- Simplifies deployment (just provide source files)
- Enables customization through URL parameters
- Reduces storage requirements

### 10. Modular Configuration

**Rationale**: Supporting CLI args, env vars, and config files:
- CLI args for quick testing and overrides
- Environment variables for containerized deployments
- Config files for complex multi-source setups
- Layered configuration with clear precedence

## Component Interactions

### PostgreSQL Integration

```mermaid
graph TB
    Martin[Martin Server]
    Pool[Connection Pool<br/>deadpool-postgres]
    
    subgraph PostgreSQL
        Tables[Tables with<br/>Geometry Columns]
        Functions[MVT Functions]
        PostGIS[PostGIS Extension]
    end
    
    Martin --> Pool
    Pool --> Tables
    Pool --> Functions
    Tables --> PostGIS
    Functions --> PostGIS
```

**How it works**:
1. Martin connects to PostgreSQL using connection string
2. Queries `geometry_columns` view to discover tables
3. Queries `pg_proc` to discover MVT-returning functions
4. Maintains connection pool for efficient query execution
5. Generates tile SQL queries with bbox parameters
6. Returns results as MVT tiles

### File Source Integration

```mermaid
graph TB
    Martin[Martin Server]
    
    subgraph "Local Files"
        MBT[MBTiles<br/>SQLite]
        PMT[PMTiles<br/>Binary Format]
        COG[GeoTIFF<br/>Cloud Optimized]
    end
    
    subgraph "Remote Files"
        S3MBT[S3/Azure/GCP<br/>MBTiles]
        S3PMT[S3/Azure/GCP<br/>PMTiles]
        S3COG[S3/Azure/GCP<br/>GeoTIFF]
    end
    
    Martin --> MBT
    Martin --> PMT
    Martin --> COG
    Martin --> S3MBT
    Martin --> S3PMT
    Martin --> S3COG
```

**How it works**:
1. Martin scans configured directories for tile files
2. Opens MBTiles with SQLite (using `sqlx`)
3. Opens PMTiles with custom parser (HTTP range requests for remote)
4. Opens COG with TIFF parser (HTTP range requests for remote)
5. Uses `object_store` crate for S3/Azure/GCP access
6. Serves tiles directly from file format

## Deployment Patterns

Martin supports multiple deployment patterns:

### 1. Standalone Server
- Single binary with embedded WebUI
- Direct PostgreSQL connection
- Local file serving
- Suitable for small to medium deployments

### 2. Container Deployment
- Docker image with all dependencies
- Configuration via environment variables
- Health check endpoints
- Suitable for Kubernetes and container orchestrators

### 3. Serverless (AWS Lambda)
- Lambda adapter for Actix-Web
- Cold start optimization
- Stateless operation
- Suitable for sporadic traffic

### 4. Behind Reverse Proxy
- NGINX or Apache fronting Martin
- Additional caching layer
- SSL termination
- Load balancing across multiple Martin instances

## Performance Characteristics

### Bottlenecks and Optimizations

**PostgreSQL Queries**:
- **Bottleneck**: Complex geometry queries on large tables
- **Optimization**: Spatial indexes (GIST), connection pooling, query tuning

**Tile Encoding**:
- **Bottleneck**: MVT encoding CPU time
- **Optimization**: Tile caching, pre-generated MBTiles for static data

**Network I/O**:
- **Bottleneck**: High tile request rate
- **Optimization**: Async I/O, connection keep-alive, compression

**Memory**:
- **Bottleneck**: Large tile cache size
- **Optimization**: LRU eviction, configurable cache size, streaming responses

### Scaling Strategies

**Vertical Scaling**:
- Increase CPU for faster tile generation
- Increase memory for larger tile cache
- Faster disk I/O for file sources

**Horizontal Scaling**:
- Run multiple Martin instances behind load balancer
- Each instance maintains its own cache
- Shared PostgreSQL database with connection pooling
- CDN for tile distribution

## Security Considerations

### Input Validation
- All tile coordinates validated (z/x/y bounds)
- SQL injection prevention through parameterized queries
- Path traversal prevention for file sources
- URL parsing with strict validation

### Database Access
- Read-only database user recommended
- Connection string security (avoid logging)
- SSL/TLS support for PostgreSQL connections
- Certificate validation for secure connections

### Network Security
- CORS configuration for cross-origin requests
- Rate limiting (via reverse proxy)
- Authentication/authorization (via reverse proxy)
- HTTPS termination (via reverse proxy)

## Extensibility Points

### Adding New Tile Sources

To add a new tile source type:

1. Implement the `Source` trait in `martin-core`
2. Add configuration parsing in `martin`
3. Register source in the catalog
4. Add integration tests

Example source types that could be added:
- Direct GeoJSON file serving
- Vector tile rendering from raster data
- Integration with other spatial databases

### Adding New Resource Types

To add new resource endpoints:

1. Implement resource generator in `martin-core`
2. Add HTTP handler in `martin/src/srv/`
3. Add configuration support
4. Update catalog/discovery

### Custom Authentication/Authorization

Martin doesn't include built-in auth, but supports:
- Reverse proxy authentication (recommended)
- Custom Actix-Web middleware
- Token-based access control via proxy

## Monitoring and Observability

### Metrics

Martin exposes Prometheus metrics via `/metrics`:
- HTTP request counters and histograms
- Tile generation time histograms
- Cache hit/miss rates
- Database connection pool stats
- Error rates by type

### Health Checks

- `/health` - Basic health check (HTTP 200)
- `/catalog` - Source availability check
- Database connection health included in metrics

### Logging

- Structured logging via `env_logger`
- Configurable log levels
- Request/response logging
- Error tracking with context

## Future Architecture Considerations

### Potential Improvements

1. **Distributed Caching**: Redis/Memcached for shared cache across instances
2. **Streaming Tile Generation**: Stream tiles as they're generated for large datasets
3. **Background Tile Pre-generation**: Queue-based tile seeding
4. **GraphQL API**: Alternative to REST for complex queries
5. **WebSocket Support**: Real-time tile updates for live data

### Backward Compatibility

Martin maintains API compatibility:
- TileJSON specification compliance
- Standard MVT tile format
- MapLibre/Mapbox GL JS compatibility
- MBTiles spec compliance

## Related Documentation

- [Configuration File](config-file.md) - Detailed configuration options
- [Development Guide](development.md) - Contributing to Martin
- [API Documentation](using.md) - HTTP API reference
- [Sources Documentation](sources-tiles.md) - Tile source configuration

## Glossary

- **MVT**: Mapbox Vector Tile - Protocol Buffer format for vector tiles
- **MBTiles**: SQLite-based archive format for tile storage
- **PMTiles**: Cloud-optimized single-file tile archive format
- **COG**: Cloud Optimized GeoTIFF - Tile-based GeoTIFF format
- **PostGIS**: Spatial extension for PostgreSQL
- **TileJSON**: JSON format describing tile sources
- **PBF**: Protocol Buffer Format - Binary serialization format
