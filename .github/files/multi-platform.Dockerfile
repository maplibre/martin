FROM alpine
ARG TARGETPLATFORM

LABEL org.opencontainers.image.description="Blazing fast and lightweight tile server with PostGIS, MBTiles, and PMTiles support"
COPY target_releases/$TARGETPLATFORM/* /usr/local/bin

HEALTHCHECK CMD wget --spider http://localhost:3000/health || exit 1
ENTRYPOINT ["/usr/local/bin/martin"]
