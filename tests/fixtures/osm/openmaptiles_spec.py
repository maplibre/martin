"""Writes openmaptiles.spec.yml: one example per assertFeatures call of Planetiler's OpenMapTiles layer tests.

It copies a Planetiler checkout (with its planetiler-openmaptiles submodule) to a temporary directory, adds
openmaptiles_spec/Recorder.java to the layer tests, patches AbstractLayerTest.java so that every processed input,
every preprocessed relation and every assertFeatures/assertCoversZoomRange call is recorded, runs the layer tests
with Maven, and turns the record into YAML. Only assertions that passed, on one input processed by the default
profile, are written; the summary of what was skipped and why goes to stderr. An input may carry `context`: the
wikidata names the test set up and the inputs it processed only for their state (e.g. Natural Earth countries).
Each osm input gets the `osm_id` and `osm_type` openmaptiles_examples.py writes it as, and `osm_geometry` when its
geometry is too big for Martin. An output's `layer` is the matched Planetiler feature's when the test leaves it out.
Run from the workspace root:

    uv run --with pyyaml --with shapely python -I tests/fixtures/osm/openmaptiles_spec.py ~/planetiler

Maven arguments after `--` go to Maven, e.g. `-- -Dmlt-ffi.version=0.2.3`; `--mvn` picks the Maven binary
(default: the checkout's ./mvnw), `--save-record FILE` keeps the record and `--record FILE` converts a saved
record without running Maven.
"""

import argparse
import collections
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import shapely
import yaml

HERE = Path(__file__).parent
SPEC = HERE / "openmaptiles.spec.yml"
RECORDER = HERE / "openmaptiles_spec" / "Recorder.java"
LAYERS = Path("planetiler-openmaptiles/src/test/java/org/openmaptiles/layers")
SKIPPED_DIRS = {"data", "target", ".git", "planet-logs", "layerstats", "node_modules"}
SQUARE_POLYGON = "POLYGON ((0 0, 1 0, 1 1, 0 1, 0 0))"
SQUARE_WAY = "LINESTRING (0 0, 1 0, 1 1, 0 1, 0 0)"
SHORT_LINE = "LINESTRING (0 0, 1 1)"

PATCHES = [
    (
        "public abstract class AbstractLayerTest {",
        "@org.junit.jupiter.api.extension.ExtendWith(Recorder.class)\npublic abstract class AbstractLayerTest {",
    ),
    (
        """final OpenMapTilesProfile profile = new OpenMapTilesProfile(translations, PlanetilerConfig.defaults(),
    Stats.inMemory());""",
        """final OpenMapTilesProfile profile = new OpenMapTilesProfile(translations, PlanetilerConfig.defaults(),
    Stats.inMemory()) {
    @Override
    public List<OsmRelationInfo> preprocessOsmRelation(
      com.onthegomap.planetiler.reader.osm.OsmElement.Relation relation) {
      return Recorder.relation(relation, super.preprocessOsmRelation(relation));
    }
  };""",
    ),
    (
        """
  static void assertFeatures(int zoom, List<Map<String, Object>> expected, Iterable<FeatureCollector.Feature> actual) {
""",
        """
  static void assertFeatures(int zoom, List<Map<String, Object>> expected, Iterable<FeatureCollector.Feature> actual) {
    Recorder.assertFeatures(zoom, expected, actual, () -> assertFeaturesUnrecorded(zoom, expected, actual));
  }

  static void assertFeaturesUnrecorded(int zoom, List<Map<String, Object>> expected,
    Iterable<FeatureCollector.Feature> actual) {
""",
    ),
    (
        """
  void assertCoversZoomRange(int minzoom, int maxzoom, String layer, FeatureCollector... featureCollectors) {
""",
        """
  void assertCoversZoomRange(int minzoom, int maxzoom, String layer, FeatureCollector... featureCollectors) {
    Recorder.assertCoversZoomRange(minzoom, maxzoom, layer, featureCollectors,
      () -> assertCoversZoomRangeUnrecorded(minzoom, maxzoom, layer, featureCollectors));
  }

  void assertCoversZoomRangeUnrecorded(int minzoom, int maxzoom, String layer,
    FeatureCollector... featureCollectors) {
""",
    ),
    (
        """
    var collector = featureCollectorFactory.get(feature);
    profile.processFeature(feature, collector);
""",
        """
    var collector = Recorder.collector(featureCollectorFactory, true, feature);
    profile.processFeature(feature, collector);
""",
    ),
]


def git_head(path):
    """The commit checked out at a path."""
    return subprocess.run(
        ["git", "-C", str(path), "rev-parse", "HEAD"], check=True, capture_output=True, text=True
    ).stdout.strip()


def copy_checkout(source, target):
    """Copies a Planetiler checkout without its data, build output and git metadata."""

    def ignore(directory, names):
        """Names to leave out of one directory."""
        return [name for name in names if name in SKIPPED_DIRS and (name != "data" or Path(directory) == source)]

    shutil.copytree(source, target, ignore=ignore, symlinks=True)


def patch_tests(checkout):
    """Adds the recorder to the layer tests of a copied checkout."""
    layers = checkout / LAYERS
    shutil.copy(RECORDER, layers / "Recorder.java")
    abstract = layers / "AbstractLayerTest.java"
    text = abstract.read_text()
    for old, new in PATCHES:
        if text.count(old) != 1:
            sys.exit(f"patch does not apply to AbstractLayerTest.java: {old[:60]!r}")
        text = text.replace(old, new)
    abstract.write_text(text)
    for test in layers.glob("*Test.java"):
        text = test.read_text()
        test.write_text(
            text.replace("featureCollectorFactory.get(", "Recorder.collector(featureCollectorFactory, false, ")
        )


def record(planetiler, mvn, mvn_args, save):
    """Runs the patched layer tests, saves the record if asked, and returns the recorded events."""
    with tempfile.TemporaryDirectory(prefix="omt-spec-") as tmp:
        checkout = Path(tmp) / "planetiler"
        copy_checkout(planetiler, checkout)
        patch_tests(checkout)
        out = Path(tmp) / "record.jsonl"
        command = [
            mvn or str(checkout / "mvnw"),
            "-q",
            "-pl",
            "planetiler-openmaptiles/submodule.pom.xml",
            "-am",
            "test",
            "-Dtest=org.openmaptiles.layers.*Test",
            "-Dsurefire.failIfNoSpecifiedTests=false",
            "-Dmaven.test.failure.ignore=true",
            "-Dspotless.check.skip=true",
            "-Dspotless.apply.skip=true",
            "-Dmaven.buildNumber.skip=true",
            *mvn_args,
        ]
        subprocess.run(command, cwd=checkout, env={**os.environ, "OMT_RECORD": str(out)}, check=True)
        if save:
            shutil.copy(out, save)
        return read_record(out)


def read_record(path):
    """Parses a JSON-lines record file."""
    return [json.loads(line) for line in Path(path).read_text().splitlines() if line.strip()]


def invocation_index(unique_id):
    """The parameterized invocation number in a JUnit unique id, or 0."""
    match = re.search(r"test-template-invocation:#(\d+)", unique_id)
    return int(match.group(1)) if match else 0


def param_of(event):
    """The parameter text of a parameterized test's display name, with Java map entries sorted, or None."""
    match = re.match(r"^\[\d+\] (.*)$", event["display"])
    if not match:
        return None
    return re.sub(r"\{([^{}]*)\}", lambda m: "{" + ", ".join(sorted(m.group(1).split(", "))) + "}", match.group(1))


def group_invocations(events):
    """Groups events by test invocation, ordered by class, method position in the file and invocation."""
    invocations = collections.OrderedDict()
    method_line = {}
    for event in events:
        if "unique_id" not in event:
            continue
        invocations.setdefault(event["unique_id"], []).append(event)
        key = (event["class"], event["method"])
        if "line" in event and event["event"] in ("assert", "zoom_range", "process"):
            method_line[key] = min(method_line.get(key, event["line"]), event["line"])

    def order(item):
        """Sort key of one invocation."""
        first = item[1][0]
        return (first["class"], method_line.get((first["class"], first["method"]), 10**9), invocation_index(item[0]))

    return sorted(invocations.items(), key=order)


def kind_of(input_event):
    """The spec's input kind of a recorded input."""
    if input_event["is_point"]:
        return "point"
    if input_event["fake_osm"] and input_event["closed_line"]:
        return "closed_way"
    if input_event["can_be_polygon"]:
        return "polygon"
    if input_event["can_be_line"]:
        return "line"
    return "unknown"


def spec_input(recorded):
    """The spec input of a recorded input, without context."""
    result = {"source": recorded["source"]}
    if recorded["source"] != "osm" and recorded.get("table") is not None:
        result["table"] = recorded["table"]
    result["kind"] = kind_of(recorded)
    result["geometry"] = recorded["geometry"]
    result["tags"] = dict(sorted(recorded["tags"].items()))
    relations = [
        {"id": r["id"], "role": r["role"], "tags": dict(sorted(r["tags"].items()))} for r in recorded["relations"]
    ]
    if relations:
        result["relations"] = relations
    return result


def plain_value(value):
    """A recorded expected value as YAML wants it: null for "<null>", text for other Java types."""
    if value == "<null>":
        return None
    if isinstance(value, dict) and "java_type" in value:
        return value["value"]
    return value


def spec_output(expected, actual, stats):
    """The spec output entries of a passed assertFeatures call."""
    entries = []
    for want, got in zip(expected, actual, strict=True):
        entry = {"layer": want.get("_layer", got["_layer"])}
        if "_layer" not in want:
            stats["layer from actual"] += 1
        if "_type" in want:
            entry["geometry"] = want["_type"]
        if "_minzoom" in want:
            entry["min_zoom"] = want["_minzoom"]
        if "_maxzoom" in want:
            entry["max_zoom"] = want["_maxzoom"]
        entry["tags"] = {k: plain_value(v) for k, v in sorted(want.items()) if not k.startswith("_")}
        extra = {
            k: plain_value(v)
            for k, v in sorted(want.items())
            if k.startswith("_") and k not in ("_layer", "_type", "_minzoom", "_maxzoom")
        }
        if extra:
            entry["planetiler"] = extra
        entries.append(entry)
    return entries


def skip_reason(event, processed):
    """Why an assertion event cannot become an example, or None."""
    collectors = [event["collector"]] if event["event"] == "assert" else event["collectors"]
    if event["event"] == "zoom_range" and len(collectors) != 1:
        return "assertCoversZoomRange over several inputs"
    if collectors[0] is None or collectors[0] not in processed:
        return "asserts on features that are not the output of one input (profile.finish)"
    if not processed[collectors[0]]["standard_profile"]:
        return "input processed by a profile with non-default config"
    if not event["passed"]:
        return "Java assertion failed"
    if any("unknown" in r for r in processed[collectors[0]]["input"]["relations"]):
        return "relation info not made by preprocessOsmRelation"
    if event["event"] == "zoom_range":
        features = event["actual"][0]
        if len(features) != 1 or features[0]["_layer"] != event["layer"]:
            return "assertCoversZoomRange input emits more than one feature"
    return None


def context_of(event_collector, processed, asserted):
    """Planetiler state an example depends on beyond its input: wikidata names and inputs processed only for state."""
    context = {}
    recorded = processed[event_collector]["input"]
    if "wikidata" in recorded:
        context["wikidata"] = {recorded["tags"]["wikidata"]: recorded["wikidata"]}
    earlier = [
        spec_input(p["input"])
        for cid, p in processed.items()
        if cid < event_collector and cid not in asserted and p["standard_profile"]
    ]
    if earlier:
        context["earlier_inputs"] = earlier
    return context


def convert(events, stats):
    """Turns recorded events into spec examples, counting recorded, converted and skipped assertions per class."""
    examples = []
    for _, group in group_invocations(events):
        processed = {e["collector"]: e for e in group if e["event"] == "process"}
        checks = [e for e in group if e["event"] in ("assert", "zoom_range")]
        asserted = {c for e in checks for c in ([e["collector"]] if e["event"] == "assert" else e["collectors"])}
        cls = group[0]["class"]
        if not checks:
            stats[cls]["tests without assertFeatures (sort keys, merges, assertEquals)"] += 1
        for n, event in enumerate(checks, start=1):
            stats[cls][f"recorded {event['event']}"] += 1
            name = f"{cls}.{event['method']}"
            if len(checks) > 1:
                name += f" #{n}"
            if param_of(event) is not None:
                name += f" {param_of(event)}"
            reason = skip_reason(event, processed)
            if reason:
                stats[cls][f"skipped: {reason}"] += 1
                if reason == "Java assertion failed":
                    stats["failed"].append(name)
                continue
            stats[cls]["converted"] += 1
            collector = event["collector"] if event["event"] == "assert" else event["collectors"][0]
            recorded = processed[collector]["input"]
            example_input = spec_input(recorded)
            if recorded["fake_osm"] is False and recorded["closed_line"]:
                stats["closed line read as line only"].append(name)
            context = context_of(collector, processed, asserted)
            if context:
                example_input["context"] = context
            if event["event"] == "assert":
                zoom = event["zoom"]
                output = spec_output(event["expected"], event["actual"], stats["all"])
            else:
                zoom = event["min_zoom"]
                output = [
                    {"layer": event["layer"], "min_zoom": event["min_zoom"], "max_zoom": event["max_zoom"], "tags": {}}
                ]
            examples.append({"name": name, "input": example_input, "at_zoom": zoom, "output": output})
    return examples


def unique_names(examples):
    """Appends the occurrence number to names that repeat."""
    seen = collections.Counter(e["name"] for e in examples)
    counts = collections.Counter()
    for example in examples:
        if seen[example["name"]] > 1:
            counts[example["name"]] += 1
            example["name"] += f" ({counts[example['name']]})"


def is_length_line(geometry):
    """Whether a line has the shape of Planetiler's lineFeatureWithLength: east from 0,0 along the equator."""
    coords = list(geometry.coords) if geometry.geom_type == "LineString" else []
    return len(coords) == 2 and coords[0] == (0.0, 0.0) and coords[1][1] == 0.0


def osm_geometry(kind, wkt, expects_features):
    """The geometry the PBF gets when the recorded one is too big for Martin, or None to keep it.

    Polygons and closed ways over 1x1 degrees become the unit square; lines over 2 degrees become a unit diagonal,
    unless the example expects features (their length may matter) or the line comes from lineFeatureWithLength.
    """
    geometry = shapely.from_wkt(wkt)
    if kind in ("polygon", "closed_way"):
        minx, miny, maxx, maxy = geometry.bounds
        if maxx - minx > 1 + 1e-9 or maxy - miny > 1 + 1e-9:
            return SQUARE_POLYGON if kind == "polygon" else SQUARE_WAY
    if kind == "line" and geometry.length > 2 and not expects_features and not is_length_line(geometry):
        return SHORT_LINE
    return None


def add_osm_ids(examples, stats):
    """Adds the OSM id, type and (if shrunk) PBF geometry of each osm example, as openmaptiles_examples.py writes."""
    for index, example in enumerate(examples):
        example_input = example["input"]
        if example_input["source"] != "osm":
            continue
        example_input["osm_id"] = index + 1
        example_input["osm_type"] = {"point": "node", "polygon": "relation"}.get(example_input["kind"], "way")
        shrunk = osm_geometry(example_input["kind"], example_input["geometry"], bool(example["output"]))
        if shrunk:
            example_input["osm_geometry"] = shrunk
            stats["shrunk"].append(example["name"])


def write_spec(examples, planetiler_commit, omt_commit):
    """Writes the spec YAML with its header."""
    header = (
        f"# Generated by tests/fixtures/osm/openmaptiles_spec.py from Planetiler {planetiler_commit} "
        f"(planetiler-openmaptiles {omt_commit}). Do not edit.\n"
    )
    body = yaml.safe_dump({"examples": examples}, sort_keys=False, allow_unicode=True, width=10**9)
    SPEC.write_text(header + body)


def report(stats):
    """Prints recorded, converted and skipped counts per test class to stderr."""
    classes = sorted(k for k in stats if k not in ("all", "failed", "shrunk", "closed line read as line only"))
    for cls in classes:
        print(f"{cls}:", file=sys.stderr)
        for key, count in sorted(stats[cls].items()):
            print(f"  {key}: {count}", file=sys.stderr)
    for key, count in sorted(stats["all"].items()):
        print(f"{key}: {count}", file=sys.stderr)
    for key in ("failed", "shrunk", "closed line read as line only"):
        print(f"{key} ({len(stats[key])}): {', '.join(stats[key])}", file=sys.stderr)


def main():
    """Records the layer tests (or reads a record) and writes the spec."""
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("planetiler", type=Path, help="Planetiler checkout with the planetiler-openmaptiles submodule")
    parser.add_argument("--mvn", help="Maven binary (default: the checkout's ./mvnw)")
    parser.add_argument("--record", type=Path, help="convert this record file instead of running the tests")
    parser.add_argument("--save-record", type=Path, help="also save the record of this run to this file")
    parser.add_argument("mvn_args", nargs="*", help="extra Maven arguments, after --")
    args = parser.parse_args()
    planetiler = args.planetiler.expanduser().resolve()
    events = read_record(args.record) if args.record else record(planetiler, args.mvn, args.mvn_args, args.save_record)
    stats = collections.defaultdict(collections.Counter)
    for key in ("failed", "shrunk", "closed line read as line only"):
        stats[key] = []
    examples = convert(events, stats)
    unique_names(examples)
    add_osm_ids(examples, stats)
    write_spec(examples, git_head(planetiler), git_head(planetiler / "planetiler-openmaptiles"))
    print(f"{len(examples)} examples written to {SPEC}", file=sys.stderr)
    report(stats)


if __name__ == "__main__":
    main()
