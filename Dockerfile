# syntax=docker/dockerfile:1

# This dockerfile must be ran with   docker buildx build

ARG TARGETPLATFORM
ARG BUILDPLATFORM

FROM --platform=$BUILDPLATFORM rust:alpine as builder
ARG TARGETPLATFORM
ARG BUILDPLATFORM

WORKDIR /usr/src/martin

RUN apk update \
    && apk add --no-cache openssl-dev musl-dev perl build-base

COPY . .
RUN if [ "$TARGETPLATFORM" = "linux/arm64" ]; then \
        echo "Building on '$BUILDPLATFORM' for ARM64"; \
        export CFLAGS=-mno-outline-atomics; \
    else \
        echo "Building on '$BUILDPLATFORM' for unrecognized target platform '$TARGETPLATFORM'"; \
    fi \
    && export CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse \
    && env | sort | tee /build_log \
    && cargo build --release --features=vendored-openssl

FROM alpine:latest

LABEL org.opencontainers.image.description="Blazing fast and lightweight tile server with PostGIS, MBTiles, and PMTiles support"

COPY --from=builder /build_log /build_log
RUN env | sort | tee /build_log2 \
    && apk add --no-cache libc6-compat

COPY --from=builder \
  /usr/src/martin/target/release/martin \
  /usr/local/bin/

EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/martin"]
