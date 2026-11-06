FROM ubuntu:24.04@sha256:68ee22004a9109b44973313b02ff7b1de250f1de0f5ecb628d73f579aaff33ac

ARG TARGETPLATFORM

# The `-full` build passes the EGL and Mesa libraries rendering needs here.
ARG EXTRA_PACKAGES=""

LABEL org.opencontainers.image.source="https://github.com/maplibre/martin"
LABEL org.opencontainers.image.licenses="Apache-2.0 OR MIT"
LABEL org.opencontainers.image.documentation="https://maplibre.org/martin/"
LABEL org.opencontainers.image.vendor="maplibre"
LABEL org.opencontainers.image.authors="Yuri Astrakhan, Stepan Kuzmin and MapLibre contributors"

# wget is needed for the healthcheck
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       wget \
       ca-certificates \
       ${EXTRA_PACKAGES} \
    && rm -rf /var/lib/apt/lists/*

COPY target_releases/$TARGETPLATFORM/* /usr/local/bin

HEALTHCHECK CMD wget --spider http://127.0.0.1:3000/health || exit 1
ENTRYPOINT ["/usr/local/bin/martin"]
