FROM postgis/postgis:18-3.6@sha256:0d513af346b21c76bf084e90c0589043c30ff379025d079ce4befa9c3a539f72

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
