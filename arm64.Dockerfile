FROM rust:1.68-bullseye as builder

WORKDIR /usr/src/martin

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
    libssl-dev \
    perl \
    && rm -rf /var/lib/apt/lists/*

COPY . .
RUN CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse cargo build --release --features=vendored-openssl


FROM debian:bullseye-slim

LABEL org.opencontainers.image.description="Blazing fast and lightweight tile server with PostGIS, MBTiles, and PMTiles support"

COPY --from=builder \
  /usr/src/martin/target/release/martin \
  /usr/local/bin/

EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/martin"]
