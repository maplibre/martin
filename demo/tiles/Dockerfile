# Stage 1: Fonts, styles — download and rewrite via Python
FROM python:3-slim AS fonts
WORKDIR /build
COPY tiles/rewrite_styles.py /build/rewrite_styles.py
RUN mkdir -p /out/styles /out/fonts && python3 /build/rewrite_styles.py

# Stage 2: Martin (final)
FROM ghcr.io/maplibre/martin:1.4.0
# Copy fonts and rewritten styles from fonts stage
COPY --from=fonts /out/styles /data/styles
COPY --from=fonts /out/fonts /data/fonts
# Sprites for demo (SDF/plain control)
COPY tiles/sprites/icons /data/sprites/icons
# Config from build context (context is demo/, so tiles/config.yaml)
COPY tiles/config.yaml /data/config.yaml

WORKDIR /data
EXPOSE 3000
CMD ["--config", "/data/config.yaml", "--webui", "enable-for-all"]
