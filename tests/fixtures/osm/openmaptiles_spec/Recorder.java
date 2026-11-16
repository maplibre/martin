package org.openmaptiles.layers;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.onthegomap.planetiler.FeatureCollector;
import com.onthegomap.planetiler.TestUtils;
import com.onthegomap.planetiler.geo.GeometryException;
import com.onthegomap.planetiler.reader.SourceFeature;
import com.onthegomap.planetiler.reader.osm.OsmElement;
import com.onthegomap.planetiler.reader.osm.OsmRelationInfo;
import com.onthegomap.planetiler.reader.osm.OsmSourceFeature;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Comparator;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.concurrent.atomic.AtomicLong;
import java.util.stream.StreamSupport;
import org.junit.jupiter.api.extension.AfterEachCallback;
import org.junit.jupiter.api.extension.BeforeEachCallback;
import org.junit.jupiter.api.extension.ExtensionContext;
import org.locationtech.jts.geom.Geometry;
import org.locationtech.jts.geom.LineString;
import org.locationtech.jts.io.WKTWriter;
import org.openmaptiles.util.Utils;

/** Records the inputs and assertions of the OpenMapTiles layer tests as JSON lines in the file $OMT_RECORD. */
public class Recorder implements BeforeEachCallback, AfterEachCallback {

  private static final ObjectMapper MAPPER = new ObjectMapper();
  private static final Path OUT = Path.of(System.getenv().getOrDefault("OMT_RECORD", "omt-record.jsonl"));
  private static final Map<OsmRelationInfo, Map<String, Object>> RELATIONS =
    Collections.synchronizedMap(new IdentityHashMap<>());
  private static final Map<FeatureCollector, Long> COLLECTORS = Collections.synchronizedMap(new IdentityHashMap<>());
  private static final AtomicLong IDS = new AtomicLong();
  private static final ThreadLocal<Map<String, Object>> TEST = new ThreadLocal<>();
  private static final ThreadLocal<AbstractLayerTest> INSTANCE = new ThreadLocal<>();

  /** Remembers which test runs on this thread. */
  @Override
  public void beforeEach(ExtensionContext context) {
    Map<String, Object> test = new LinkedHashMap<>();
    test.put("class", context.getRequiredTestClass().getSimpleName());
    test.put("method", context.getRequiredTestMethod().getName());
    test.put("display", context.getDisplayName());
    test.put("unique_id", context.getUniqueId());
    TEST.set(test);
    INSTANCE.set((AbstractLayerTest) context.getRequiredTestInstance());
  }

  /** Writes whether the test passed. */
  @Override
  public void afterEach(ExtensionContext context) {
    Map<String, Object> event = event("test");
    event.put("passed", context.getExecutionException().isEmpty());
    write(event);
    TEST.remove();
    INSTANCE.remove();
  }

  /** Remembers the raw tags of the relation behind each relation info it was preprocessed into. */
  static List<OsmRelationInfo> relation(OsmElement.Relation relation, List<OsmRelationInfo> infos) {
    if (infos != null) {
      for (var info : infos) {
        Map<String, Object> raw = new LinkedHashMap<>();
        raw.put("id", relation.id());
        raw.put("tags", plain(new TreeMap<>(relation.tags())));
        RELATIONS.put(info, raw);
      }
    }
    return infos;
  }

  /** Creates a feature collector and records the feature it is for. */
  static FeatureCollector collector(FeatureCollector.Factory factory, boolean standardProfile, SourceFeature feature) {
    var collector = factory.get(feature);
    long id = IDS.incrementAndGet();
    COLLECTORS.put(collector, id);
    Map<String, Object> event = event("process");
    event.put("collector", id);
    event.put("standard_profile", standardProfile);
    event.put("input", describe(feature));
    write(event);
    return collector;
  }

  /** Records an assertFeatures call, then runs it. */
  static void assertFeatures(int zoom, List<Map<String, Object>> expected, Iterable<FeatureCollector.Feature> actual,
    Runnable assertion) {
    Map<String, Object> event = event("assert");
    event.put("zoom", zoom);
    event.put("collector", actual instanceof FeatureCollector c ? COLLECTORS.get(c) : null);
    event.put("expected", expected.stream()
      .sorted(Comparator.comparing(d -> Utils.coalesce(d.get("_layer"), "").toString()))
      .map(Recorder::plain).toList());
    event.put("actual", StreamSupport.stream(actual.spliterator(), false)
      .sorted(Comparator.comparing(FeatureCollector.Feature::getLayer))
      .map(f -> summary(f, zoom)).toList());
    run(event, assertion);
  }

  /** Records an assertCoversZoomRange call, then runs it. */
  static void assertCoversZoomRange(int minzoom, int maxzoom, String layer, FeatureCollector[] collectors,
    Runnable assertion) {
    Map<String, Object> event = event("zoom_range");
    event.put("min_zoom", minzoom);
    event.put("max_zoom", maxzoom);
    event.put("layer", layer);
    List<Object> collectorIds = new ArrayList<>();
    List<Object> actual = new ArrayList<>();
    for (var collector : collectors) {
      collectorIds.add(COLLECTORS.get(collector));
      actual.add(StreamSupport.stream(collector.spliterator(), false).map(f -> summary(f, minzoom)).toList());
    }
    event.put("collectors", collectorIds);
    event.put("actual", actual);
    run(event, assertion);
  }

  /** Runs an assertion and writes the event with its outcome. */
  private static void run(Map<String, Object> event, Runnable assertion) {
    try {
      assertion.run();
      event.put("passed", true);
    } catch (AssertionError e) {
      event.put("passed", false);
      event.put("error", String.valueOf(e.getMessage()));
      throw e;
    } finally {
      write(event);
    }
  }

  /** Returns a feature's layer, geometry type and zoom range. */
  private static Map<String, Object> summary(FeatureCollector.Feature feature, int zoom) {
    var map = TestUtils.toMap(feature, zoom);
    Map<String, Object> result = new LinkedHashMap<>();
    for (var key : List.of("_layer", "_type", "_minzoom", "_maxzoom")) {
      result.put(key, plain(map.get(key)));
    }
    return result;
  }

  /** Describes a source feature: source, geometry, tags and relations. */
  private static Map<String, Object> describe(SourceFeature feature) {
    Map<String, Object> input = new LinkedHashMap<>();
    Geometry geometry;
    try {
      geometry = feature.latLonGeometry();
    } catch (GeometryException e) {
      throw new IllegalStateException(e);
    }
    input.put("source", feature.getSource());
    input.put("table", feature.getSourceLayer());
    input.put("fake_osm", feature instanceof OsmSourceFeature);
    input.put("is_point", feature.isPoint());
    input.put("can_be_line", feature.canBeLine());
    input.put("can_be_polygon", feature.canBePolygon());
    input.put("closed_line", geometry instanceof LineString line && line.isClosed());
    input.put("geometry_type", geometry.getGeometryType());
    input.put("geometry", new WKTWriter().write(geometry));
    input.put("id", feature.id());
    input.put("tags", plain(new TreeMap<>(feature.tags())));
    List<Object> relations = new ArrayList<>();
    for (var member : feature.relationInfo(OsmRelationInfo.class)) {
      var raw = RELATIONS.get(member.relation());
      Map<String, Object> relation = new LinkedHashMap<>();
      if (raw == null) {
        relation.put("unknown", member.relation().getClass().getName() + " " + member.relation());
      } else {
        relation.putAll(raw);
      }
      relation.put("role", member.role());
      relations.add(relation);
    }
    input.put("relations", relations);
    var wikidata = feature.tags().get("wikidata");
    var test = INSTANCE.get();
    if (wikidata instanceof String qid && qid.startsWith("Q") && test != null) {
      try {
        var names = test.wikidataTranslations.get(Long.parseLong(qid.substring(1)));
        if (names != null && !names.isEmpty()) {
          input.put("wikidata", new TreeMap<>(names));
        }
      } catch (NumberFormatException e) {
        input.put("wikidata_error", qid);
      }
    }
    return input;
  }

  /** Converts a value to JSON-friendly types, keeping numbers and booleans. */
  private static Object plain(Object value) {
    if (value == null || value instanceof String || value instanceof Number || value instanceof Boolean) {
      return value;
    }
    if (value instanceof Map<?, ?> map) {
      Map<String, Object> result = new LinkedHashMap<>();
      map.forEach((k, v) -> result.put(String.valueOf(k), plain(v)));
      return result;
    }
    if (value instanceof List<?> list) {
      return list.stream().map(Recorder::plain).toList();
    }
    Map<String, Object> other = new LinkedHashMap<>();
    other.put("java_type", value.getClass().getName());
    other.put("value", String.valueOf(value));
    return other;
  }

  /** Starts an event for the current test with its call site in the test class. */
  private static Map<String, Object> event(String kind) {
    Map<String, Object> event = new LinkedHashMap<>();
    event.put("event", kind);
    var test = TEST.get();
    if (test != null) {
      event.putAll(test);
    }
    for (var frame : Thread.currentThread().getStackTrace()) {
      String name = frame.getClassName();
      if (name.startsWith("org.openmaptiles.layers.") && !name.endsWith(".Recorder") &&
        !name.endsWith(".AbstractLayerTest")) {
        event.put("line", frame.getLineNumber());
        event.put("frame", name.substring(name.lastIndexOf('.') + 1) + "." + frame.getMethodName());
        break;
      }
    }
    return event;
  }

  /** Appends one JSON line to the record file. */
  private static synchronized void write(Map<String, Object> event) {
    try {
      Files.writeString(OUT, MAPPER.writeValueAsString(event) + "\n", StandardCharsets.UTF_8,
        StandardOpenOption.CREATE, StandardOpenOption.APPEND);
    } catch (IOException e) {
      throw new UncheckedIOException(e);
    }
  }
}
