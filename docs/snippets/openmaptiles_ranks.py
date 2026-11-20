"""Prints the `rules` of the `place_country` layer of `openmaptiles.yaml`: each country code's rank and minzoom.

Planetiler's OpenMapTiles profile ranks a country label by the Natural Earth country polygon it lies in: the average of
its `scalerank` and `labelrank`, rounded up and clamped to 1..6. This writes the same ranks keyed by the country code,
from Natural Earth's 1:10m Admin 0 Countries (https://www.naturalearthdata.com/downloads/10m-cultural-vectors/), so the
configuration needs no Natural Earth data. Codes Natural Earth lacks (`-99`) fall back to `ISO_A2_EH`.

    curl -LO https://naciscdn.org/naturalearth/10m/cultural/ne_10m_admin_0_countries.zip
    uv run --with pyshp python docs/snippets/openmaptiles_ranks.py ne_10m_admin_0_countries.zip
"""

import io
import math
import sys
import textwrap
import zipfile

import shapefile


def countries(path):
    """Yields each country of the Natural Earth zip as a dict of its attributes."""
    archive = zipfile.ZipFile(path)
    dbf = next(name for name in archive.namelist() if name.endswith(".dbf"))
    reader = shapefile.Reader(dbf=io.BytesIO(archive.read(dbf)), encoding="utf-8")
    fields = [field[0].lower() for field in reader.fields[1:]]
    for record in reader.records():
        yield dict(zip(fields, record))


def ranks(path):
    """Returns the rank of each country code, as Planetiler's `Place` layer computes it."""
    result = {}
    for country in countries(path):
        code = country["iso_a2"] if country["iso_a2"] != "-99" else country["iso_a2_eh"]
        if code == "-99" or code in result:
            continue
        rank = math.ceil((country["scalerank"] + country["labelrank"]) / 2)
        result[code] = min(max(rank, 1), 6)
    return result


def main():
    """Prints one rule per rank from 1 to 5, indented to paste under the layer; rank 6 is the layer's own setting."""
    by_rank = {}
    for code, rank in sorted(ranks(sys.argv[1]).items()):
        by_rank.setdefault(rank, []).append(f"'{code}'")
    iso = "coalesce(country_code_iso3166_1_alpha_2, feature['ISO3166-1:alpha2'], feature['ISO3166-1'])"
    for rank in range(1, 6):
        where = textwrap.wrap(f"{iso} in [{', '.join(by_rank[rank])}]", width=100)
        print("            - where: >-")
        print("\n".join(f"                {line}" for line in where))
        print(f"              minzoom: {rank - 1}")
        print(f"              attributes: {{ rank: {rank} }}")


if __name__ == "__main__":
    main()
