# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `AsyncCogReader`: read `WebMercatorQuad` tiles from one tile-aligned COG, passing JPEG/WebP tiles
  through and re-encoding lossless 8-bit tiles as PNG.
- Backends: `MemoryBackend`, `FileBackend` (`fs`), `ObjectStoreBackend` (`object-store`).
