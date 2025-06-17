# (1) this stage will be run always on current arch
# zigbuild & Cargo targets added
FROM --platform=$BUILDPLATFORM rust:1.87-alpine AS chef
WORKDIR /app
ENV PKGCONFIG_SYSROOTDIR=/
RUN apk add --no-cache musl-dev nodejs npm openssl-dev zig
RUN cargo install --locked cargo-chef cargo-zigbuild
RUN rustup target add aarch64-unknown-linux-musl x86_64-unknown-linux-musl

# (2) preparing recipe file
FROM chef AS planner
# copies sorted by approx. change frequency
COPY martin-tile-utils martin-tile-utils
COPY mbtiles mbtiles
COPY martin martin
COPY Cargo.* .
RUN cargo chef prepare --recipe-path recipe.json

# (3) building project deps: need to specify all targets; zigbuild used
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --recipe-path recipe.json --release --zigbuild \
    --target x86_64-unknown-linux-musl \
    --target aarch64-unknown-linux-musl

# (4) actuall project build for all targets
# copies sorted by approx. change frequency
COPY logo.png .
COPY martin-tile-utils martin-tile-utils
COPY mbtiles mbtiles
COPY martin martin
COPY Cargo.* .
RUN cargo zigbuild --release \
    --target aarch64-unknown-linux-musl \
    --target x86_64-unknown-linux-musl
# binary renamed to easier copy in runtime stage
RUN mkdir /app/linux && \
    for bin in martin martin-cp mbtiles; do \
        mv target/aarch64-unknown-linux-musl/release/$bin /app/linux/arm64; \
        mv target/x86_64-unknown-linux-musl/release/$bin /app/linux/amd64; \
    done

# (5) runtime image
# TARGETPLATFORM usage to copy right binary from builder stage
# ARG populated by docker itself
FROM alpine:3.22 AS runtime

ARG TARGETPLATFORM
COPY --from=builder /app/$TARGETPLATFORM/* /usr/local/bin

HEALTHCHECK CMD wget --spider http://127.0.0.1:3000/health || exit 1
ENTRYPOINT ["/usr/local/bin/martin"]
