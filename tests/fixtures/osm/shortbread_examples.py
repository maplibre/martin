"""Writes shortbread-examples.osm.pbf: one OSM object per example of Planetiler's shortbread.spec.yml.

Example `i` (from 0, in file order) is OSM id `i + 1`: a node for `point`, an open way for `line`, a closed way for
`polygon`, or a `type=boundary` relation of one closed way for a `polygon` with a `boundary` tag, as OSM has them.
Each geometry is 8 pixels wide at the lowest zoom its outputs expect, so `min_size` keeps it there.
Examples of other sources than `osm` are left out.

    uv run --with osmium --with pyyaml python tests/fixtures/osm/shortbread_examples.py
"""

import re
from pathlib import Path

import osmium
import yaml

HERE = Path(__file__).parent
MEMBER_WAYS = 100_000
NODES = 1_000_000


def size(example):
    """Degrees 8 pixels span at the lowest zoom of the example's outputs."""
    outputs = example["output"]
    outputs = outputs if isinstance(outputs, list) else [outputs]
    zoom = min((int(o.get("min_zoom", 0)) for o in outputs), default=14)
    return round(8 * 360 / (256 * 2**zoom), 7)


def coords(example):
    """The example's points as lon/lat."""
    geometry = example["input"]["geometry"]
    side = size(example)
    if geometry == "point":
        return [(1, 1)]
    if geometry == "line":
        return [(1, 1), (1 + side, 1)]
    if geometry == "polygon":
        return [(1, 1), (1 + side, 1), (1 + side, 1 + side), (1, 1 + side), (1, 1)]
    numbers = [float(n) for n in re.findall(r"-?\d+(?:\.\d+)?", geometry)]
    return list(zip(numbers[::2], numbers[1::2]))


def main():
    spec = yaml.load((HERE / "shortbread.spec.yml").read_text(), Loader=yaml.BaseLoader)
    nodes, ways, relations = [], [], []
    for index, example in enumerate(spec["examples"]):
        if example["input"]["source"] != "osm":
            continue
        osm_id = index + 1
        tags = example["input"].get("tags") or {}
        points = coords(example)
        if len(points) == 1:
            nodes.append(osmium.osm.mutable.Node(id=osm_id, location=points[0], tags=tags))
            continue
        refs = []
        for n, point in enumerate(points[:-1] if points[0] == points[-1] else points):
            refs.append(NODES + osm_id * 10 + n)
            nodes.append(osmium.osm.mutable.Node(id=refs[-1], location=point))
        if points[0] == points[-1]:
            refs.append(refs[0])
        if points[0] == points[-1] and "boundary" in tags:
            ways.append(osmium.osm.mutable.Way(id=MEMBER_WAYS + osm_id, nodes=refs))
            members = [("w", MEMBER_WAYS + osm_id, "outer")]
            relations.append(osmium.osm.mutable.Relation(id=osm_id, members=members, tags={**tags, "type": "boundary"}))
        else:
            ways.append(osmium.osm.mutable.Way(id=osm_id, nodes=refs, tags=tags))
    out = HERE / "shortbread-examples.osm.pbf"
    out.unlink(missing_ok=True)
    writer = osmium.SimpleWriter(str(out))
    for node in sorted(nodes, key=lambda n: n.id):
        writer.add_node(node)
    for way in sorted(ways, key=lambda w: w.id):
        writer.add_way(way)
    for relation in sorted(relations, key=lambda r: r.id):
        writer.add_relation(relation)
    writer.close()


if __name__ == "__main__":
    main()
