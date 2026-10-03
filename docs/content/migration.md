---
icon: material/arrow-up-bold
tags:
  - getting-started
  - deployment
---

# Migration Guide

## Migrating to 2.0

### Minimum x86_64 CPU is now x86-64-v3

The prebuilt x86_64 binaries, Debian package and Docker images are built for the x86-64-v3 feature level (AVX2, BMI2, FMA; Intel Haswell / AMD Excavator, 2015 or newer).
On older CPUs they crash with an illegal instruction error.

If you need to run Martin on an older CPU, [build it from source](development/index.md) without `-C target-cpu=x86-64-v3`.
aarch64 builds are unaffected.
