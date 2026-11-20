#![cfg(feature = "osm")]
#![expect(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;

use martin::generate::spec::backend::TestBackend as _;
use martin::generate::spec::check;
use martin::generate::spec::file::{Attribute, Output, read_spec};
use martin::generate::spec::osm::Osm;
use martin::generate::spec::render::{outputs, render_bulk};
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
    output: OneOrMany,
}

#[derive(Deserialize)]
struct Input {
    source: String,
    geometry: String,
    #[serde(default)]
    tags: Option<BTreeMap<String, Value>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(Expected),
    Many(Vec<Expected>),
}

#[derive(Deserialize)]
struct Expected {
    layer: Option<String>,
    geometry: Option<String>,
    min_zoom: Option<u8>,
    max_zoom: Option<u8>,
    at_zoom: Option<u8>,
    allow_extra_tags: Option<bool>,
    #[serde(default)]
    tags: Option<BTreeMap<String, Value>>,
}

fn attributes_at(output: &Output, zoom: Option<u8>) -> BTreeMap<String, Value> {
    let zoom = zoom.unwrap_or(u8::MAX).clamp(output.zooms.min, output.zooms.max);
    output
        .attributes
        .iter()
        .filter_map(|(key, attribute)| match attribute {
            Attribute::Value(value) => Some((key.clone(), value.clone())),
            Attribute::Zoomed {
                value,
                minzoom,
                maxzoom,
            } => (minzoom.unwrap_or(output.zooms.min) <= zoom && zoom <= maxzoom.unwrap_or(output.zooms.max))
                .then(|| (key.clone(), value.clone())),
        })
        .collect()
}

fn same_value(expected: &Value, actual: &Value) -> bool {
    match (expected.as_f64(), actual.as_f64()) {
        (Some(want), Some(got)) if expected.is_f64() => (want - got).abs() <= want.abs() * 1e-12,
        _ => expected == actual,
    }
}

fn proximity(expected: &Expected, output: &Output) -> usize {
    let attributes = attributes_at(output, expected.at_zoom);
    let tags = expected.tags.iter().flatten();
    usize::from(expected.layer.as_ref() == Some(&output.layer)) * 2
        + usize::from(expected.geometry.as_ref() == Some(&output.geometry.to_string()))
        + tags
            .map(|(key, value)| match attributes.get(key) {
                Some(actual) if same_value(value, actual) => 2,
                Some(_) => 1,
                None => 0,
            })
            .sum::<usize>()
}

fn issues(expected: &[Expected], mut actual: Vec<Output>) -> Vec<String> {
    let mut issues = Vec::new();
    let mut expected: Vec<&Expected> = expected.iter().collect();
    expected.retain(|want| {
        let Some(zoom) = want.at_zoom.filter(|_| {
            want.layer.is_none()
                && want.geometry.is_none()
                && want.min_zoom.is_none()
                && want.max_zoom.is_none()
                && want.tags.is_none()
        }) else {
            return true;
        };
        match actual
            .iter()
            .position(|got| !(got.zooms.min..=got.zooms.max).contains(&zoom))
        {
            Some(hidden) => drop(actual.remove(hidden)),
            None => issues.push(format!(
                "no feature at z{zoom}, got {:?}",
                actual.iter().map(|o| &o.layer).collect::<Vec<_>>()
            )),
        }
        false
    });
    if !issues.is_empty() {
        return issues;
    }
    if expected.len() != actual.len() {
        let layers = actual.iter().map(|o| o.layer.as_str()).collect::<Vec<_>>();
        return vec![format!(
            "expected {} features, got {} {layers:?}",
            expected.len(),
            actual.len()
        )];
    }
    for want in expected {
        let best = (0..actual.len()).max_by_key(|&i| proximity(want, &actual[i])).unwrap();
        let got = actual.remove(best);
        let layer = want.layer.as_deref().unwrap_or("?");
        if want.layer.as_ref().is_some_and(|want| *want != got.layer) {
            issues.push(format!("layer {layer}, got {}", got.layer));
        }
        if let Some(geometry) = &want.geometry
            && *geometry != got.geometry.to_string()
        {
            issues.push(format!("{layer} geometry {geometry}, got {}", got.geometry));
        }
        if let Some(min) = want.min_zoom
            && min != got.zooms.min
        {
            issues.push(format!("{layer} min_zoom {min}, got {}", got.zooms.min));
        }
        if let Some(max) = want.max_zoom
            && max != got.zooms.max
        {
            issues.push(format!("{layer} max_zoom {max}, got {}", got.zooms.max));
        }
        let mut attributes = attributes_at(&got, want.at_zoom);
        for (key, value) in want.tags.iter().flatten() {
            match attributes.remove(key) {
                Some(actual) if same_value(value, &actual) => {}
                Some(actual) => issues.push(format!("{layer}.{key} {value}, got {actual}")),
                None => issues.push(format!("{layer}.{key} {value}, got none")),
            }
        }
        if want.allow_extra_tags == Some(false) {
            for (key, value) in attributes {
                issues.push(format!("{layer}.{key} not expected, got {value}"));
            }
        }
    }
    issues
}

#[tokio::test]
async fn shortbread_snippet_renders_planetiler_examples() {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/osm");
    let snippet = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/snippets/shortbread.yaml"))
        .unwrap()
        .replace(
            "path: berlin-latest.osm.pbf",
            &format!("path: {fixtures}/shortbread-examples.osm.pbf"),
        );
    let mut config = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
    config.write_all(snippet.as_bytes()).unwrap();
    let backend = Osm::load(config.path()).unwrap().unwrap();
    let tables = backend.tables(14).unwrap();
    let spec_yaml = std::fs::read_to_string(format!("{fixtures}/shortbread.spec.yml")).unwrap();
    let spec: Spec =
        serde_saphyr::from_str_with_options(&spec_yaml, serde_saphyr::options! { strict_booleans: true }).unwrap();
    let mut report = String::new();
    let mut matched = 0;
    let mut osm = 0;
    for (index, example) in spec.examples.iter().enumerate() {
        if example.input.source != "osm" {
            continue;
        }
        osm += 1;
        let tags = example.input.tags.clone().unwrap_or_default();
        let kind = match example.input.geometry.as_str() {
            "point" => 1,
            "polygon" if tags.contains_key("boundary") => 3,
            geometry if geometry.starts_with("POLYGON") && tags.contains_key("boundary") => 3,
            _ => 2,
        };
        let key = (index as u64 + 1) * 10 + kind;
        let (tiles, _) = render_bulk(&backend, &tables[0], key).await.unwrap();
        let expected = match &example.output {
            OneOrMany::One(one) => std::slice::from_ref(one),
            OneOrMany::Many(many) => many.as_slice(),
        };
        let mut actual = outputs(&tiles);
        let polygon = example.input.geometry == "polygon" || example.input.geometry.starts_with("POLYGON");
        if kind == 2 && polygon && tags.get("area") != Some(&Value::from("yes")) {
            actual.retain(|output| output.geometry.to_string() != "line");
        }
        let issues = issues(expected, actual);
        if issues.is_empty() {
            matched += 1;
        } else {
            writeln!(report, "{}:", example.name).unwrap();
            for issue in issues {
                writeln!(report, "  {issue}").unwrap();
            }
        }
    }
    writeln!(report, "{matched} of {osm} examples match").unwrap();
    insta::assert_snapshot!(report, @"
    66 of 66 examples match
    ");
}

#[tokio::test]
async fn shortbread_snippet_passes_its_monaco_spec() {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/osm");
    let snippet = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/snippets/shortbread.yaml"))
        .unwrap()
        .replace(
            "path: berlin-latest.osm.pbf",
            &format!("path: {fixtures}/monaco.osm.pbf"),
        );
    let mut config = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
    config.write_all(snippet.as_bytes()).unwrap();
    let backend = Osm::load(config.path()).unwrap().unwrap();
    let spec = read_spec(format!("{fixtures}/shortbread-monaco.spec.yaml").as_ref()).unwrap();
    let tables = backend.tables(spec.max_zoom).unwrap();
    let mut report = String::new();
    let mut passed = 0;
    for case in &spec.cases {
        let outcome = check(&backend, &tables, case).await.unwrap();
        if outcome.row.is_none() && outcome.output.is_none() {
            passed += 1;
            continue;
        }
        writeln!(report, "{} {:?}: {}", case.input["id"], outcome.status(), case.path()).unwrap();
        if let Some(output) = outcome.output {
            writeln!(
                report,
                "{}",
                similar::TextDiff::from_lines(&output.old, &output.new).unified_diff()
            )
            .unwrap();
        }
    }
    writeln!(report, "{passed} of {} cases pass", spec.cases.len()).unwrap();
    insta::assert_snapshot!(report, @"51 of 51 cases pass");
}
