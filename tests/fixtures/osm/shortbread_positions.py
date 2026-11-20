"""Writes shortbread-positions.osm.pbf: ways whose point in Shortbread's point layers depends on their shape.

Way 1 is an L-shaped building with a house number, way 2 an open way tagged `amenity=bench`, and way 3 a U-shaped
closed way tagged `amenity=school`. The centroids of ways 1 and 3 lie outside them.

    uv run --with osmium python tests/fixtures/osm/shortbread_positions.py
"""

from pathlib import Path

import osmium

HERE = Path(__file__).parent
NODES = 1_000

WAYS = {
    1: (
        {"building": "yes", "addr:housenumber": "1"},
        [(1.000, 1.000), (1.004, 1.000), (1.004, 1.001), (1.001, 1.001), (1.001, 1.004), (1.000, 1.004), (1.000, 1.000)],
    ),
    2: (
        {"amenity": "bench"},
        [(1.010, 1.000), (1.014, 1.000)],
    ),
    3: (
        {"amenity": "school"},
        [
            (1.020, 1.000),
            (1.021, 1.000),
            (1.021, 1.003),
            (1.023, 1.003),
            (1.023, 1.000),
            (1.024, 1.000),
            (1.024, 1.004),
            (1.020, 1.004),
            (1.020, 1.000),
        ],
    ),
}


def main():
    nodes, ways = [], []
    for way_id, (tags, points) in WAYS.items():
        closed = points[0] == points[-1]
        refs = []
        for n, point in enumerate(points[:-1] if closed else points):
            refs.append(NODES + way_id * 10 + n)
            nodes.append(osmium.osm.mutable.Node(id=refs[-1], location=point))
        if closed:
            refs.append(refs[0])
        ways.append(osmium.osm.mutable.Way(id=way_id, nodes=refs, tags=tags))
    path = HERE / "shortbread-positions.osm.pbf"
    path.unlink(missing_ok=True)
    writer = osmium.SimpleWriter(str(path))
    for node in sorted(nodes, key=lambda n: n.id):
        writer.add_node(node)
    for way in ways:
        writer.add_way(way)
    writer.close()


if __name__ == "__main__":
    main()
