FROM alpine
ARG TARGETPLATFORM

LABEL org.opencontainers.image.description="Blazing fast and lightweight tile server with PostGIS, MBTiles, and PMTiles support"
COPY target_releases/$TARGETPLATFORM/* /usr/local/bin

ENTRYPOINT ["/usr/local/bin/martin"]
