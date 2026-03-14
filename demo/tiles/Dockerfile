# Stage 1: Planetiler — download Oberbayern and build oberbayern.pmtiles (MLT, Z0-13)
FROM ghcr.io/onthegomap/planetiler:latest AS planetiler
WORKDIR /build
# Use same invocation as image entrypoint: java -cp @/app/jib-classpath-file com.onthegomap.planetiler.Main
ENV JAVA_TOOL_OPTIONS="-Xmx2g"
RUN java -cp @/app/jib-classpath-file com.onthegomap.planetiler.Main \
  --download --download-threads=10 --download-chunk-size-mb=1000 \
  --free_natural_earth_after_read=true --free_water_polygons_after_read=true --free_lake_centerlines_after_read=true \
  --languages=en \
  --area=oberbayern --bound=oberbayern \
  --output=data/oberbayern.pmtiles \
  --tile_format=mlt --tile_compression=none \
  --mlt_advanced=true --mlt_fastpfor=true --mlt_fsst=true \
  --mlt_reorder_features=true --mlt_tessellate_polygons=false --mlt_shared_dict=false \
  --exclude_ids=true --skip_filled_tiles=true --transportation_name_brunnel=true \
  --exclude_layers=building,housenumber,poi,transportation_name,water_name,aerodrome_label,aeroway,mountain_peak,place,park \
  --min_feature_size=2 --min_feature_size_at_max_zoom=0.25 \
  --simplify_tolerance=0.2 --simplify_tolerance_at_max_zoom=0.125 \
  --max_zoom=13

# Stage 2: Fonts, styles — download and rewrite via Python (no apt-get; use python image)
FROM python:3-slim AS fonts
WORKDIR /build
COPY tiles/rewrite_styles.py /build/rewrite_styles.py
RUN mkdir -p /out/styles /out/fonts && python3 /build/rewrite_styles.py

# Stage 3: Martin (final) — no wget, only COPY from previous stages
FROM ghcr.io/maplibre/martin:1.3.1
# Copy pmtiles from planetiler stage
COPY --from=planetiler /build/data/oberbayern.pmtiles /data/oberbayern.pmtiles
# Copy fonts and rewritten styles from fonts stage
COPY --from=fonts /out/styles /data/styles
COPY --from=fonts /out/fonts /data/fonts
# Config from build context (context is demo/, so tiles/config.yaml)
COPY tiles/config.yaml /data/config.yaml

WORKDIR /data
EXPOSE 3000
CMD ["--config", "/data/config.yaml", "--webui", "enable-for-all"]
