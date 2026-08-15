FROM ubuntu:24.04@sha256:561618e2c15bf2397621dd04f96926663a3b5616c189cf7e38db7e82f5c538ea

ARG TARGETPLATFORM

LABEL org.opencontainers.image.description="Blazing fast and lightweight tile server with PostGIS, MBTiles, and PMTiles support"
LABEL org.opencontainers.image.source="https://github.com/maplibre/martin"
LABEL org.opencontainers.image.licenses="Apache-2.0 OR MIT"
LABEL org.opencontainers.image.documentation="https://maplibre.org/martin/"
LABEL org.opencontainers.image.vendor="maplibre"
LABEL org.opencontainers.image.authors="Yuri Astrakhan, Stepan Kuzmin and MapLibre contributors"

# Install runtime dependencies for the rendering feature (maplibre_native needs Vulkan/Mesa, libcurl, libuv,
# plus image codec and ICU shared libraries that the maplibre_native pre-built shared object links against).
# wget is needed for the healthcheck
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       mesa-vulkan-drivers \
       libcurl4 \
       libglfw3 \
       libicu74 \
       libjpeg-turbo8 \
       libpng16-16t64 \
       libuv1 \
       libwebp7 \
       wget \
       ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY target_releases/$TARGETPLATFORM/* /usr/local/bin

HEALTHCHECK CMD wget --spider http://127.0.0.1:3000/health || exit 1
ENTRYPOINT ["/usr/local/bin/martin"]
