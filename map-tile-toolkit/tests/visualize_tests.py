"""Regenerate a QGIS project from scratch: one layer per GeoJSON file under `tests/`.

The layer tree mirrors the directory tree: one group per directory, sorted, holding its files
(sorted) above its subdirectories, so `grid.geojson` sits on top. Every layer is a plain single
symbol in a stable color derived from its path, except the grid, which is styled by its `role`
(tile edges, buffer edges, the zero axes). A file holding several geometry types becomes one layer
per type, as QGIS itself does. The project is in EPSG:4326 and opens on the fixture grid; layer
sources are relative to the project file, so it can be written anywhere.

Everything is rebuilt on each run; only the view state of an existing project is kept - which
groups and layers are checked or expanded, and the map extent. Delete the file for a pristine one.
Needs only the standard library.

Usage: python3 tests/visualize_tests.py path/to/project.qgz
"""

import colorsys
import os
import hashlib
import json
import sys
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

TESTS = Path(__file__).resolve().parent
if len(sys.argv) != 2:
    sys.exit(f"usage: python3 {sys.argv[0]} path/to/project.qgz")
QGZ = Path(sys.argv[1]).resolve()
QGS_NAME = QGZ.with_suffix(".qgs").name
QGIS_VERSION = "4.2.2-Belém do Pará"

# GeoJSON geometry type -> (QGIS geometry kind, OGR sublayer name)
KINDS = {
    "Point": ("Point", "Point"),
    "MultiPoint": ("Point", "Point"),
    "LineString": ("Line", "LineString"),
    "MultiLineString": ("Line", "LineString"),
    "Polygon": ("Polygon", "Polygon"),
    "MultiPolygon": ("Polygon", "Polygon"),
}

WGS84 = {
    "proj4": "+proj=longlat +datum=WGS84 +no_defs",
    "srsid": "3452",
    "srid": "4326",
    "authid": "EPSG:4326",
    "description": "WGS 84",
    "projectionacronym": "longlat",
    "ellipsoidacronym": "EPSG:7030",
    "geographicflag": "true",
}

OUTLINE = "35,35,35,255"

# The grid's line style per `role`, as `SimpleLine` options: thin lines sized in screen pixels, so
# they stay hairlines at any zoom - dotted red buffer edges, dashed dark tile edges, blue zero axes.
GRID_PIXEL_UNITS = {
    f"{k}_unit": "Pixel" for k in ("line_width", "customdash", "dash_pattern_offset", "offset", "trim_distance_start", "trim_distance_end")
}
GRID_ROLES = [
    ("buffer", {"line_color": "236,1,1,255", "line_style": "dot", "line_width": "0.7", "customdash": "1;4", "capstyle": "square", "joinstyle": "bevel"}),
    ("grid", {"line_color": "35,35,35,255", "line_style": "dash", "line_width": "1", "customdash": "2;5", "capstyle": "round", "joinstyle": "round"}),
    ("zero", {"line_color": "72,123,182,255", "line_style": "solid", "line_width": "2", "customdash": "0.66;2", "capstyle": "round", "joinstyle": "round"}),
]


def relative_source(path):
    """`path` relative to the project file, as QGIS stores a relative layer source."""
    rel = Path(os.path.relpath(path, QGZ.parent)).as_posix()
    return rel if rel.startswith("../") else "./" + rel


GRID_SOURCE = relative_source(TESTS / "grid.geojson")


def walk_coords(coords, out):
    if coords and isinstance(coords[0], (int, float)):
        out.append(coords[:2])
    else:
        for c in coords or []:
            walk_coords(c, out)


def file_layers(path):
    """One `(source, kind, wkb, extent)` per geometry kind in the file, points first."""
    data = json.loads(path.read_text())
    features = data.get("features", []) if data.get("type") == "FeatureCollection" else [data]
    by_kind = {}
    for f in features:
        geom = f.get("geometry") or {}
        if geom.get("type") not in KINDS:
            continue
        kind, sublayer = KINDS[geom["type"]]
        entry = by_kind.setdefault(kind, {"sublayer": sublayer, "types": set(), "coords": []})
        entry["types"].add(geom["type"])
        walk_coords(geom.get("coordinates"), entry["coords"])
    rel = relative_source(path)
    layers = []
    for kind in ("Point", "Line", "Polygon"):
        if kind not in by_kind:
            continue
        e = by_kind[kind]
        source = rel if len(by_kind) == 1 else f"{rel}|geometrytype={e['sublayer']}"
        multi = any(t.startswith("Multi") for t in e["types"])
        wkb = ("Multi" if multi else "") + e["sublayer"]
        xs = [c[0] for c in e["coords"]] or [0]
        ys = [c[1] for c in e["coords"]] or [0]
        layers.append((source, kind, wkb, (min(xs), min(ys), max(xs), max(ys))))
    return layers


def color_for(source):
    """A stable, moderately saturated color per source, like QGIS's random layer colors."""
    h = int(hashlib.md5(source.encode()).hexdigest()[:8], 16)
    r, g, b = colorsys.hsv_to_rgb((h % 360) / 360, 0.5 + (h >> 9) % 30 / 100, 0.75 + (h >> 17) % 20 / 100)
    return ",".join(str(round(c * 255)) for c in (r, g, b)) + ",255"


def layer_id(source):
    """A stable layer id: the file name plus a hash of the whole source."""
    name = Path(source.split("|")[0]).name.removesuffix(".geojson")
    digest = hashlib.md5(source.encode()).hexdigest()[:12]
    return name.replace("-", "_").replace(".", "_") + "_" + digest


def layer_name(source):
    return Path(source.split("|")[0]).name.removesuffix(".geojson")


def sub(parent, tag, text=None, **attrib):
    e = ET.SubElement(parent, tag, attrib)
    if text is not None:
        e.text = text
    return e


def spatialrefsys(parent):
    sys_ = sub(parent, "spatialrefsys", nativeFormat="Wkt")
    for k, v in WGS84.items():
        sub(sys_, k, v)


def srs(parent, tag):
    spatialrefsys(sub(parent, tag))


def extent(parent, tag, box):
    e = sub(parent, tag)
    for name, value in zip(("xmin", "ymin", "xmax", "ymax"), box):
        sub(e, name, f"{value:g}")


def options(parent, values):
    m = sub(parent, "Option", type="Map")
    for k, v in values.items():
        sub(m, "Option", name=k, type="QString", value=v)


def line_options(color, width):
    return {
        "capstyle": "square",
        "joinstyle": "bevel",
        "line_color": color,
        "line_style": "solid",
        "line_width": width,
        "line_width_unit": "MM",
    }


def symbol(parent, kind, name, color, opts=None):
    """A one-layer symbol of the given layer kind (`Point`, `Line`, or `Polygon`)."""
    sym_type, cls = {"Point": ("marker", "SimpleMarker"), "Line": ("line", "SimpleLine"), "Polygon": ("fill", "SimpleFill")}[kind]
    s = sub(parent, "symbol", alpha="1", clip_to_extent="1", force_rhr="0", name=name, type=sym_type)
    if opts is None:
        opts = {
            "Point": {
                "name": "circle",
                "color": color,
                "size": "2",
                "size_unit": "MM",
                "outline_color": OUTLINE,
                "outline_style": "solid",
                "outline_width": "0",
                "outline_width_unit": "MM",
            },
            "Line": line_options(color, "0.26"),
            "Polygon": {
                "color": color,
                "style": "solid",
                "outline_color": OUTLINE,
                "outline_style": "solid",
                "outline_width": "0.26",
                "outline_width_unit": "MM",
                "joinstyle": "bevel",
            },
        }[kind]
    options(ET.SubElement(s, "layer", {"class": cls, "enabled": "1", "locked": "0", "pass": "0"}), opts)
    return s


def single_renderer(parent, kind, color):
    r = sub(parent, "renderer-v2", type="singleSymbol", enableorderby="0", forceraster="0", symbollevels="0", referencescale="-1")
    symbol(sub(r, "symbols"), kind, "0", color)


def grid_renderer(parent):
    """The grid's lines categorized by `role`, anything else in the default thin line."""
    r = sub(parent, "renderer-v2", type="categorizedSymbol", attr="role", enableorderby="0", forceraster="0", symbollevels="0", referencescale="-1")
    cats, syms = sub(r, "categories"), sub(r, "symbols")
    for i, (role, style) in enumerate(GRID_ROLES):
        sub(cats, "category", label=role, render="true", symbol=str(i), type="string", value=role)
        symbol(syms, "Line", str(i), style["line_color"], {**style, **GRID_PIXEL_UNITS, "use_custom_dash": "1"})
    other = str(len(GRID_ROLES))
    sub(cats, "category", label="", render="true", symbol=other, type="NULL", value="NULL")
    symbol(syms, "Line", other, "80,73,219,255")


def map_layer(parent, source, kind, wkb, box):
    m = sub(parent, "maplayer", type="vector", geometry=kind, wkbType=wkb, layerType="Vector", hasScaleBasedVisibilityFlag="0", minScale="100000000", maxScale="0", readOnly="0", styleCategories="AllStyleCategories", labelsEnabled="0")
    extent(m, "extent", box)
    extent(m, "wgs84extent", box)
    sub(m, "id", layer_id(source))
    sub(m, "datasource", source)
    sub(m, "layername", layer_name(source))
    srs(m, "srs")
    sub(m, "provider", "ogr", encoding="UTF-8")
    if source == GRID_SOURCE:
        grid_renderer(m)
    else:
        single_renderer(m, kind, color_for(source))
    sub(m, "blendMode", "0")
    sub(m, "featureBlendMode", "0")
    sub(m, "layerOpacity", "1")


def read_view_state(path):
    """From an existing project: `(group states by path, layer states by source, canvas extent)`,
    each state a `(checked, expanded)` pair."""
    if not path.exists():
        return {}, {}, None
    try:
        with zipfile.ZipFile(path) as z:
            root = ET.fromstring(z.read(next(n for n in z.namelist() if n.endswith(".qgs"))))
    except (zipfile.BadZipFile, StopIteration, ET.ParseError) as e:
        print(f"ignoring the view state of {path.name}: {e}")
        return {}, {}, None
    groups, layers = {}, {}

    def walk(group, prefix):
        for g in group.findall("layer-tree-group"):
            p = prefix + (g.get("name"),)
            groups[p] = (g.get("checked"), g.get("expanded"))
            walk(g, p)
        for n in group.findall("layer-tree-layer"):
            layers[n.get("source")] = (n.get("checked"), n.get("expanded"))

    tree = root.find("layer-tree-group")
    if tree is not None:
        walk(tree, ())
    e = root.find("mapcanvas/extent")
    box = None if e is None else tuple(float(e.find(k).text) for k in ("xmin", "ymin", "xmax", "ymax"))
    return groups, layers, box


def scan():
    """The GeoJSON files as a directory tree: `{"files": [(source, kind, wkb, extent)], "dirs": {}}`."""
    tree = {"files": [], "dirs": {}}
    for path in sorted(TESTS.rglob("*.geojson"), key=lambda p: p.relative_to(TESTS).parts):
        rel = path.relative_to(TESTS).parts
        if any(part.startswith(".") for part in rel) or "target" in rel:
            continue
        node = tree
        for part in rel[:-1]:
            node = node["dirs"].setdefault(part, {"files": [], "dirs": {}})
        node["files"].extend(file_layers(path))
    return tree


def main():
    old_groups, old_layers, old_extent = read_view_state(QGZ)
    tree = scan()

    root = ET.Element("qgis", projectname="", version=QGIS_VERSION)
    sub(root, "homePath", path="")
    sub(root, "title")
    srs(root, "projectCrs")
    layer_tree = sub(root, "layer-tree-group")
    sub(sub(layer_tree, "customproperties"), "Option")
    canvas = sub(root, "mapcanvas", name="theMapCanvas", annotationsVisible="1")
    sub(canvas, "units", "degrees")
    project_layers = sub(root, "projectlayers")
    layer_order = sub(root, "layerorder")

    order, boxes = [], []

    def build(node, path, parent):
        for source, kind, wkb, box in node["files"]:
            map_layer(project_layers, source, kind, wkb, box)
            order.append(layer_id(source))
            boxes.append(box)
            checked, expanded = old_layers.get(source, ("Qt::Checked", "0"))
            n = sub(parent, "layer-tree-layer", checked=checked, expanded=expanded, id=layer_id(source), name=layer_name(source), providerKey="ogr", source=source, legend_exp="", legend_split_behavior="0", patch_size="-1,-1")
            sub(sub(n, "customproperties"), "Option")
        for name in sorted(node["dirs"]):
            p = path + (name,)
            # New groups start hidden and collapsed, so a refresh never floods the map.
            checked, expanded = old_groups.get(p, ("Qt::Unchecked", "0"))
            g = sub(parent, "layer-tree-group", checked=checked, expanded=expanded, groupLayer="", name=name)
            g.set("wms-group-request-mode", "Normal")
            sub(sub(g, "customproperties"), "Option")
            build(node["dirs"][name], p, g)

    build(tree, (), layer_tree)
    for i in order:
        sub(layer_order, "layer", id=i)

    if old_extent is None:
        grid = next((f[3] for f in tree["files"] if f[0] == GRID_SOURCE), None)
        xs = [b[0] for b in boxes] + [b[2] for b in boxes]
        ys = [b[1] for b in boxes] + [b[3] for b in boxes]
        lo_x, lo_y, hi_x, hi_y = grid or (min(xs), min(ys), max(xs), max(ys))
        pad = 0.05 * max(hi_x - lo_x, hi_y - lo_y)
        old_extent = (lo_x - pad, lo_y - pad, hi_x + pad, hi_y + pad)
    extent(canvas, "extent", old_extent)
    sub(canvas, "rotation", "0")
    srs(canvas, "destinationsrs")

    view = sub(root, "ProjectViewSettings", UseProjectScales="0", rotation="0")
    sub(view, "Scales")
    corners = dict(zip(("xmin", "ymin", "xmax", "ymax"), (f"{v:g}" for v in old_extent)))
    spatialrefsys(sub(view, "DefaultViewExtent", **corners))

    props = sub(root, "properties", name="properties")
    paths = sub(props, "properties", name="Paths")
    sub(paths, "properties", "false", name="Absolute", type="bool")
    crs = sub(props, "properties", name="SpatialRefSys")
    sub(crs, "properties", "1", name="ProjectionsEnabled", type="int")

    ET.indent(root, space="  ")
    xml = ET.tostring(root, encoding="unicode")
    with zipfile.ZipFile(QGZ, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr(QGS_NAME, "<!DOCTYPE qgis PUBLIC 'http://mrcc.com/qgis.dtd' 'SYSTEM'>\n" + xml + "\n")

    print(f"{QGZ.name}: {len(order)} layers")


if __name__ == "__main__":
    main()
