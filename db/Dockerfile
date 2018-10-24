FROM postgres:10

ENV PG_MAJOR=10
ENV POSTGIS_MAJOR=2.4

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  postgresql-$PG_MAJOR-postgis-$POSTGIS_MAJOR \
  postgresql-$PG_MAJOR-postgis-$POSTGIS_MAJOR-scripts \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
