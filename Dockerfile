FROM rust:alpine as builder

WORKDIR /usr/src/martin

RUN apk update \
    && apk add --no-cache openssl-dev musl-dev perl build-base

COPY . .
RUN CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse cargo build --release --features=vendored-openssl


FROM alpine:latest

LABEL org.opencontainers.image.description="Blazing fast and lightweight tile server with PostGIS, MBTiles, and PMTiles support"

RUN apk add --no-cache libc6-compat

COPY --from=builder \
  /usr/src/martin/target/release/martin \
  /usr/local/bin/

EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/martin"]
