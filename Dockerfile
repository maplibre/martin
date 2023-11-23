FROM rust:1-bookworm as builder

WORKDIR /usr/src/martin

COPY . .
RUN cargo build --release


FROM debian:bookworm

LABEL org.opencontainers.image.description="Blazing fast and lightweight tile server with PostGIS, MBTiles, and PMTiles support"

COPY --from=builder \
  /usr/src/martin/target/release/martin \
  /usr/local/bin/

EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/martin"]
