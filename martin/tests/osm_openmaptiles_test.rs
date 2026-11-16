#![cfg(all(feature = "osm", feature = "shapefile"))]
#![expect(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;

use futures::StreamExt as _;
use martin::generate::spec::backend::TestBackend as _;
use martin::generate::spec::files::Files;
use martin::generate::spec::render::{Feature, Tiles, render_bulk};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Spec {
    examples: Vec<Example>,
}

#[derive(Deserialize)]
struct Example {
    name: String,
    input: Input,
    at_zoom: u8,
    output: Vec<Expected>,
}

#[derive(Deserialize)]
struct Input {
    source: String,
    #[serde(default)]
    table: Option<String>,
    #[serde(default)]
    osm_id: Option<u64>,
    #[serde(default)]
    osm_type: Option<String>,
}

#[derive(Deserialize)]
struct Expected {
    layer: Option<String>,
    geometry: Option<String>,
    min_zoom: Option<u8>,
    max_zoom: Option<u8>,
    #[serde(default)]
    tags: BTreeMap<String, Value>,
}

struct Output<'t> {
    layer: &'t str,
    geometry: String,
    copy: usize,
    zooms: BTreeMap<u8, &'t Feature>,
}

impl Output<'_> {
    fn min_zoom(&self) -> u8 {
        *self.zooms.keys().next().unwrap()
    }

    fn max_zoom(&self) -> u8 {
        *self.zooms.keys().next_back().unwrap()
    }

    fn attributes_at(&self, zoom: u8) -> &BTreeMap<String, Value> {
        let zoom = zoom.clamp(self.min_zoom(), self.max_zoom());
        let (_, feature) = self.zooms.range(..=zoom).next_back().unwrap();
        &feature.attributes
    }
}

fn outputs(tiles: &Tiles) -> Vec<Output<'_>> {
    let mut outputs: Vec<Output<'_>> = Vec::new();
    for (coord, features) in tiles {
        let mut in_tile: Vec<&Feature> = Vec::new();
        for feature in features {
            let geometry = feature.kind().to_string();
            let same = |other: &&Feature| other.layer == feature.layer && other.kind().to_string() == geometry;
            if in_tile
                .iter()
                .any(|other| same(other) && other.id == feature.id && other.attributes == feature.attributes)
            {
                continue;
            }
            let copy = in_tile.iter().filter(|other| same(other)).count();
            in_tile.push(feature);
            let found = outputs
                .iter()
                .position(|output| output.layer == feature.layer && output.geometry == geometry && output.copy == copy);
            let index = found.unwrap_or_else(|| {
                outputs.push(Output {
                    layer: &feature.layer,
                    geometry,
                    copy,
                    zooms: BTreeMap::new(),
                });
                outputs.len() - 1
            });
            outputs[index].zooms.entry(coord.z()).or_insert(feature);
        }
    }
    outputs
}

fn same_value(expected: &Value, actual: &Value) -> bool {
    match (expected.as_f64(), actual.as_f64()) {
        (Some(want), Some(got)) => (want - got).abs() <= want.abs() * 1e-9,
        _ => expected == actual,
    }
}

fn proximity(expected: &Expected, output: &Output<'_>, zoom: u8) -> usize {
    let attributes = output.attributes_at(zoom);
    usize::from(expected.layer.as_deref() == Some(output.layer)) * 4
        + usize::from(expected.geometry.as_deref() == Some(output.geometry.as_str())) * 2
        + expected
            .tags
            .iter()
            .map(|(key, value)| match attributes.get(key) {
                Some(actual) if same_value(value, actual) => 2,
                Some(_) => 1,
                None => usize::from(value.is_null()),
            })
            .sum::<usize>()
}

fn issues(expected: &[Expected], mut actual: Vec<Output<'_>>, zoom: u8) -> Vec<String> {
    if expected.len() != actual.len() {
        let got: Vec<String> = actual
            .iter()
            .map(|output| format!("{} {}", output.layer, output.geometry))
            .collect();
        return vec![format!("expected {} features, got {got:?}", expected.len())];
    }
    let mut issues = Vec::new();
    for want in expected {
        let best = (0..actual.len())
            .max_by_key(|&i| proximity(want, &actual[i], zoom))
            .unwrap();
        let got = actual.remove(best);
        let layer = want.layer.as_deref().unwrap_or("?");
        if want.layer.as_deref().is_some_and(|want| want != got.layer) {
            issues.push(format!("layer {layer}, got {}", got.layer));
        }
        if let Some(geometry) = &want.geometry
            && *geometry != got.geometry
        {
            issues.push(format!("{layer} geometry {geometry}, got {}", got.geometry));
        }
        if let Some(min) = want.min_zoom
            && min != got.min_zoom()
        {
            issues.push(format!("{layer} min_zoom {min}, got {}", got.min_zoom()));
        }
        if let Some(max) = want.max_zoom
            && max != got.max_zoom()
        {
            issues.push(format!("{layer} max_zoom {max}, got {}", got.max_zoom()));
        }
        let attributes = got.attributes_at(zoom);
        for (key, value) in &want.tags {
            match (attributes.get(key), value.is_null()) {
                (None, true) => {}
                (Some(actual), true) => issues.push(format!("{layer}.{key} none, got {actual}")),
                (Some(actual), false) if same_value(value, actual) => {}
                (Some(actual), false) => issues.push(format!("{layer}.{key} {value}, got {actual}")),
                (None, false) => issues.push(format!("{layer}.{key} {value}, got none")),
            }
        }
    }
    issues
}

#[tokio::test]
#[cfg_attr(
    debug_assertions,
    ignore = "takes minutes without optimizations; `just test-omt` runs it in release"
)]
async fn openmaptiles_snippet_renders_planetiler_examples() {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures");
    let snippet = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/snippets/openmaptiles.yaml"
    ))
    .unwrap()
    .replace(
        "path: berlin-latest.osm.pbf",
        &format!("path: {fixtures}/osm/openmaptiles-examples.osm.pbf"),
    )
    .replace(
        "path: water-polygons-split-3857.zip",
        &format!("path: {fixtures}/shapefile/ocean.zip"),
    );
    let mut config = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
    config.write_all(snippet.as_bytes()).unwrap();
    let backend = Files::load(config.path()).unwrap().unwrap();
    let tables = backend.tables(14).unwrap();
    let spec_yaml = std::fs::read_to_string(format!("{fixtures}/osm/openmaptiles.spec.yml")).unwrap();
    let spec: Spec =
        serde_saphyr::from_str_with_options(&spec_yaml, serde_saphyr::options! { strict_booleans: true }).unwrap();
    let threads = std::thread::available_parallelism().unwrap().get();
    let results: Vec<Vec<String>> = futures::stream::iter(&spec.examples)
        .map(|example| {
            let backend = &backend;
            let tables = &tables;
            async move {
                let (source, key) = match example.input.source.as_str() {
                    "osm" => {
                        let kind = match example.input.osm_type.as_deref() {
                            Some("node") => 1,
                            Some("way") => 2,
                            _ => 3,
                        };
                        ("openmaptiles", example.input.osm_id.unwrap() * 10 + kind)
                    }
                    "water_polygons" => ("ocean", 1),
                    source => {
                        let table = example.input.table.as_deref().unwrap_or_default();
                        return vec![format!("{source} {table} is not read")];
                    }
                };
                let table = tables.iter().find(|table| table.source == source).unwrap();
                let (tiles, _) = render_bulk(backend, table, key).await.unwrap();
                issues(&example.output, outputs(&tiles), example.at_zoom)
            }
        })
        .buffered(threads)
        .collect()
        .await;
    let mut report = String::new();
    let mut matched = 0;
    for (example, issues) in spec.examples.iter().zip(results) {
        if issues.is_empty() {
            matched += 1;
        } else {
            writeln!(report, "{}:", example.name).unwrap();
            for issue in issues {
                writeln!(report, "  {issue}").unwrap();
            }
        }
    }
    writeln!(report, "{matched} of {} examples match", spec.examples.len()).unwrap();
    insta::assert_snapshot!(report);
}
