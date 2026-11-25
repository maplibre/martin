"""Writes openmaptiles-examples.osm.pbf: one OSM object per osm example of openmaptiles.spec.yml.

Example `i` (from 0, in file order) is the OSM object `osm_id = i + 1` of type `osm_type`, both in the spec: a node
for `point`, an open way for `line`, a closed way for `closed_way`, or a `type=multipolygon` relation of one untagged
outer way for `polygon` (a `boundary` example keeps its own `type`). The geometry is the spec's `osm_geometry` when
openmaptiles_spec.py shrank a too-big input, else the recorded `geometry`. The `k`-th smallest recorded relation id of
the example becomes relation `RELATIONS + i * 100 + k`, with the recorded tags and one member per recorded membership,
in their order. Examples of other sources are left out.

    uv run --with osmium --with pyyaml --with shapely python -I tests/fixtures/osm/openmaptiles_examples.py
"""

import sys
from pathlib import Path

import osmium
import shapely
import yaml

HERE = Path(__file__).parent
MEMBER_WAYS = 100_000
RELATIONS = 100_000
NODES = 1_000_000
MEMBER_TYPES = {"node": "n", "way": "w", "relation": "r"}


def osm_value(value):
    """An OSM tag value for a spec tag value, as Java's String.valueOf writes it."""
    if isinstance(value, bool):
        return "true" if value else "false"
    return str(value)


def osm_tags(tags):
    """OSM tags for spec tags."""
    return {key: osm_value(value) for key, value in (tags or {}).items()}


def way_nodes(osm_id, coords, nodes):
    """Adds the nodes of a way's points and returns the way's node refs, closing rings on their first node."""
    closed = len(coords) > 2 and coords[0] == coords[-1]
    refs = []
    for n, point in enumerate(coords[:-1] if closed else coords):
        if n >= 100:
            sys.exit(f"example {osm_id} has more than 100 nodes")
        refs.append(NODES + osm_id * 100 + n)
        nodes.append(osmium.osm.mutable.Node(id=refs[-1], location=point))
    return refs + refs[:1] if closed else refs


def polygon_tags(tags):
    """Tags of the multipolygon relation of a polygon example."""
    if "boundary" in tags and "type" in tags:
        return tags
    return {**tags, "type": "multipolygon"}


def add_example(index, example, nodes, ways, relations):
    """Adds the OSM objects of one example."""
    example_input = example["input"]
    osm_id, osm_type, kind = example_input["osm_id"], example_input["osm_type"], example_input["kind"]
    if osm_id != index + 1:
        sys.exit(f"{example['name']}: osm_id {osm_id} is not {index + 1}")
    tags = osm_tags(example_input["tags"])
    geometry = shapely.from_wkt(example_input.get("osm_geometry") or example_input["geometry"])
    if kind == "point" and osm_type == "node":
        nodes.append(osmium.osm.mutable.Node(id=osm_id, location=(geometry.x, geometry.y), tags=tags))
    elif kind in ("line", "closed_way") and osm_type == "way" and geometry.geom_type == "LineString":
        ways.append(osmium.osm.mutable.Way(id=osm_id, nodes=way_nodes(osm_id, list(geometry.coords), nodes), tags=tags))
    elif kind == "polygon" and osm_type == "relation" and geometry.geom_type == "Polygon" and not geometry.interiors:
        refs = way_nodes(osm_id, list(geometry.exterior.coords), nodes)
        ways.append(osmium.osm.mutable.Way(id=MEMBER_WAYS + osm_id, nodes=refs))
        members = [("w", MEMBER_WAYS + osm_id, "outer")]
        relations.append(osmium.osm.mutable.Relation(id=osm_id, members=members, tags=polygon_tags(tags)))
    else:
        sys.exit(f"{example['name']}: cannot write {kind} {geometry.geom_type} as {osm_type}")
    recorded = example_input.get("relations") or []
    replacement = {old: RELATIONS + index * 100 + k for k, old in enumerate(sorted({r["id"] for r in recorded}))}
    members, tags = {}, {}
    for relation in recorded:
        members.setdefault(relation["id"], []).append((MEMBER_TYPES[osm_type], osm_id, relation["role"]))
        tags[relation["id"]] = osm_tags(relation["tags"])
    for old, new in replacement.items():
        relations.append(osmium.osm.mutable.Relation(id=new, members=members[old], tags=tags[old]))


def main():
    """Reads the spec and writes the PBF, sorted by type and id."""
    spec = yaml.safe_load((HERE / "openmaptiles.spec.yml").read_text())
    nodes, ways, relations = [], [], []
    for index, example in enumerate(spec["examples"]):
        if example["input"]["source"] != "osm":
            continue
        if "osm_geometry" in example["input"]:
            print(f"shrunk: {example['name']}", file=sys.stderr)
        add_example(index, example, nodes, ways, relations)
    out = HERE / "openmaptiles-examples.osm.pbf"
    out.unlink(missing_ok=True)
    writer = osmium.SimpleWriter(str(out))
    for node in sorted(nodes, key=lambda n: n.id):
        writer.add_node(node)
    for way in sorted(ways, key=lambda w: w.id):
        writer.add_way(way)
    for relation in sorted(relations, key=lambda r: r.id):
        writer.add_relation(relation)
    writer.close()
    print(f"{len(nodes)} nodes, {len(ways)} ways, {len(relations)} relations written to {out}", file=sys.stderr)


if __name__ == "__main__":
    main()
