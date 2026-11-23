#![cfg(feature = "osm")]
#![expect(clippy::unwrap_used)]

use std::io::Write as _;

use indoc::indoc;
use martin::generate::spec::backend::TestBackend as _;
use martin::generate::spec::file::yaml;
use martin::generate::spec::osm::Osm;
use martin::generate::spec::report::Reporter;
use martin::generate::spec::rows::RowSearch;
use martin::generate::spec::{check, review};

/// The OSM backend of the configuration `yaml`, its `${MONACO}` the Monaco extract of the test fixtures.
fn load(yaml: &str) -> Osm {
    let monaco = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/osm/monaco.osm.pbf");
    let mut file = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
    file.write_all(yaml.replace("${MONACO}", monaco).as_bytes()).unwrap();
    Osm::load(file.path()).unwrap().unwrap()
}

#[tokio::test]
async fn osm_selects_an_object_on_each_path_of_a_where() {
    let backend = load(indoc! {"
        osm:
          sources:
            monaco:
              path: ${MONACO}
              tags: [amenity]
              layers:
                cafes:
                  geometry: point
                  where: amenity == 'cafe'
                  attributes: []
    "});
    let review = review(&backend, 14, None, 0, &Reporter::new()).await.unwrap();
    let found: Vec<(String, RowSearch)> = review
        .found
        .iter()
        .map(|found| (found.branch.to_string(), found.row.clone()))
        .collect();
    insta::assert_debug_snapshot!(found, @r#"
    [
        (
            "cafes: kept by `where`",
            Selected {
                rows: "geometry is a Point AND amenity = 'cafe'",
                key: 9179787631,
            },
        ),
        (
            "cafes: dropped, `where` is false",
            Selected {
                rows: "geometry is a Point AND amenity IS DISTINCT FROM 'cafe'",
                key: 219118861,
            },
        ),
        (
            "cafes: dropped, `amenity` is null in `where`",
            Shared(
                219118861,
            ),
        ),
    ]
    "#);
}

#[tokio::test]
async fn osm_generate_writes_a_case_per_object_found() {
    let backend = load(indoc! {"
        osm:
          sources:
            monaco:
              path: ${MONACO}
              tags: [highway, name]
              layers:
                roads:
                  geometry: line
                  minzoom: \"highway == 'primary' ? 10 : 14\"
                  where: highway in ['primary', 'residential']
                  attributes: [highway, name]
    "});
    let mut review = review(&backend, 14, None, 0, &Reporter::new()).await.unwrap();
    review.accept_all();
    insta::assert_snapshot!(yaml(&review.spec()), @r#"
    max_zoom: 14
    cases:
    - source: monaco
      paths:
      - "roads: kept by `where`"
      - "roads: minzoom 10, `highway == 'primary'` is true"
      - "roads: crosses a z14 tile edge"
      input:
        geometry: LINESTRING(7.4259518 43.7389494,7.4258602 43.7389997,7.4257964 43.739037,7.425743 43.7390849,7.4257083 43.7391243,7.4256818 43.7391636,7.4256611 43.7392238,7.4256536 43.7392768,7.4256563 43.7393298,7.4256802 43.7393746,7.4257083 43.7394273,7.4257117 43.7394671,7.4256947 43.7395025,7.4256666 43.7395255,7.4256203 43.7395471,7.4251395 43.7397104)
        highway: primary
        id: 40976562
        name: Avenue Princesse Alice
      output:
      - layer: roads
        id: 40976562
        geometry: line
        zooms: 11-14
        attributes:
          highway: primary
          name: Avenue Princesse Alice
    - source: monaco
      paths:
      - "roads: dropped, `where` is false"
      - "roads: dropped, `highway` is null in `where`"
      input:
        geometry: POLYGON((7.4153692 43.7270029,7.4163084 43.7277046,7.4156948 43.7281335,7.4147556 43.7274319,7.4153692 43.7270029))
        highway: null
        id: 42249782
        name: null
      output: []
    - source: monaco
      paths:
      - "roads: minzoom 14, `highway == 'primary'` is false"
      - "roads: kept by `where`"
      input:
        geometry: LINESTRING(7.4174924 43.7296303,7.4174638 43.7296727,7.4174191 43.7297138,7.4173286 43.7297725,7.4172044 43.7298528)
        highway: residential
        id: 42249722
        name: Avenue des Papalins
      output:
      - layer: roads
        id: 42249722
        geometry: line
        zooms: 14-14
        attributes:
          highway: residential
          name: Avenue des Papalins
    - source: monaco
      paths:
      - "roads: kept only at z14 (`min_size_at_maxzoom`)"
      - "roads: kept by `where`"
      - "roads: minzoom 10, `highway == 'primary'` is true"
      input:
        geometry: LINESTRING(7.4229689 43.7323358,7.4229781 43.7323174,7.4229897 43.7322588)
        highway: primary
        id: 42271642
        name: Virage Antony Nogues
      output:
      - layer: roads
        id: 42271642
        geometry: line
        zooms: 14-14
        attributes:
          highway: primary
          name: Virage Antony Nogues
    "#);
}

#[tokio::test]
async fn osm_cases_of_a_generated_spec_pass() {
    let backend = load(indoc! {"
        osm:
          sources:
            monaco:
              path: ${MONACO}
              tags: [highway, name]
              layers:
                roads:
                  geometry: line
                  minzoom: \"highway == 'primary' ? 10 : 14\"
                  where: highway in ['primary', 'residential']
                  attributes: [highway, name]
    "});
    let mut review = review(&backend, 14, None, 0, &Reporter::new()).await.unwrap();
    review.accept_all();
    let spec = review.spec();
    let tables = backend.tables(spec.max_zoom).unwrap();
    let mut outcomes = Vec::new();
    for case in &spec.cases {
        outcomes.push(check(&backend, &tables, case).await.unwrap().status());
    }
    insta::assert_debug_snapshot!(outcomes, @"
    [
        Pass,
        Pass,
        Pass,
        Pass,
    ]
    ");
}

#[tokio::test]
async fn osm_generate_writes_one_case_for_the_polygon_and_the_outline_of_a_closed_way() {
    let backend = load(indoc! {"
        osm:
          sources:
            monaco:
              path: ${MONACO}
              tags: [aeroway, name]
              layers:
                helipad_outlines:
                  geometry: line
                  where: aeroway == 'helipad' && name == '1'
                  attributes: [name]
                helipads:
                  geometry: polygon
                  where: aeroway == 'helipad' && name == '1'
                  attributes: [name]
    "});
    let mut review = review(&backend, 14, None, 0, &Reporter::new()).await.unwrap();
    review.accept_all();
    insta::assert_snapshot!(yaml(&review.spec()), @r#"
    max_zoom: 14
    cases:
    - source: monaco
      paths:
      - "helipad_outlines: kept by `where`"
      - "helipads: kept by `where`"
      input:
        aeroway: helipad
        geometry: POLYGON((7.4193719 43.7254854,7.4193949 43.7255006,7.4194098 43.7255204,7.4194151 43.7255426,7.4194101 43.7255649,7.4193954 43.7255848,7.4193726 43.7256002,7.4193441 43.7256094,7.419313 43.7256114,7.4192825 43.725606,7.4192562 43.7255938,7.4192367 43.725576,7.4192264 43.7255546,7.4192263 43.7255319,7.4192364 43.7255104,7.4192556 43.7254925,7.4192818 43.7254801,7.4193122 43.7254745,7.4193433 43.7254763,7.4193719 43.7254854))
        id: 5729327562
        name: "1"
      output:
      - layer: helipad_outlines
        id: 5729327562
        geometry: line
        zooms: 13-14
        attributes:
          name: "1"
      - layer: helipads
        id: 5729327562
        geometry: polygon
        zooms: 13-14
        attributes:
          name: "1"
    - source: monaco
      paths:
      - "helipad_outlines: dropped, `where` is false"
      - "helipad_outlines: dropped, `aeroway` is null in `where`"
      input:
        aeroway: null
        geometry: LINESTRING(7.4259518 43.7389494,7.4258602 43.7389997,7.4257964 43.739037,7.425743 43.7390849,7.4257083 43.7391243,7.4256818 43.7391636,7.4256611 43.7392238,7.4256536 43.7392768,7.4256563 43.7393298,7.4256802 43.7393746,7.4257083 43.7394273,7.4257117 43.7394671,7.4256947 43.7395025,7.4256666 43.7395255,7.4256203 43.7395471,7.4251395 43.7397104)
        id: 40976562
        name: Avenue Princesse Alice
      output: []
    - source: monaco
      paths:
      - "helipad_outlines: dropped, `name` is null in `where`"
      - "helipads: dropped, `where` is false"
      - "helipads: dropped, `aeroway` is null in `where`"
      - "helipads: dropped, `name` is null in `where`"
      - "helipad_outlines: dropped, `where` is false"
      - "helipad_outlines: dropped, `aeroway` is null in `where`"
      input:
        aeroway: null
        geometry: POLYGON((7.4153692 43.7270029,7.4163084 43.7277046,7.4156948 43.7281335,7.4147556 43.7274319,7.4153692 43.7270029))
        id: 42249782
        name: null
      output: []
    "#);
}

#[tokio::test]
async fn osm_cases_of_a_generated_spec_with_closed_ways_pass() {
    let backend = load(indoc! {"
        osm:
          sources:
            monaco:
              path: ${MONACO}
              tags: [aeroway, name]
              layers:
                helipad_outlines:
                  geometry: line
                  where: aeroway == 'helipad'
                  attributes: [name]
                helipads:
                  geometry: polygon
                  where: aeroway == 'helipad'
                  attributes: [name]
    "});
    let mut review = review(&backend, 14, None, 0, &Reporter::new()).await.unwrap();
    review.accept_all();
    let spec = review.spec();
    let tables = backend.tables(spec.max_zoom).unwrap();
    let mut outcomes = Vec::new();
    for case in &spec.cases {
        outcomes.push(check(&backend, &tables, case).await.unwrap().status());
    }
    insta::assert_debug_snapshot!(outcomes, @"
    [
        Pass,
        Pass,
        Pass,
        Pass,
    ]
    ");
}

#[test]
fn osm_source_reading_the_tag_id_is_refused() {
    let monaco = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/osm/monaco.osm.pbf");
    let mut file = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
    let yaml = indoc! {"
        osm:
          sources:
            monaco:
              path: ${MONACO}
              tags: [id, name]
              layers:
                names:
                  attributes: [name]
    "};
    file.write_all(yaml.replace("${MONACO}", monaco).as_bytes()).unwrap();
    let error = Osm::load(file.path()).err().unwrap();
    insta::assert_snapshot!(error, @"Source `monaco` reads the tag `id`, which `martin test` reserves in each case's input");
}

#[test]
fn osm_source_reading_the_tag_geometry_is_refused() {
    let monaco = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/osm/monaco.osm.pbf");
    let mut file = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
    let yaml = indoc! {"
        osm:
          sources:
            monaco:
              path: ${MONACO}
              tags: [name, geometry]
              layers:
                names:
                  attributes: [name]
    "};
    file.write_all(yaml.replace("${MONACO}", monaco).as_bytes()).unwrap();
    let error = Osm::load(file.path()).err().unwrap();
    insta::assert_snapshot!(error, @"Source `monaco` reads the tag `geometry`, which `martin test` reserves in each case's input");
}

#[test]
fn osm_and_database_sources_together_are_refused() {
    let monaco = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/osm/monaco.osm.pbf");
    let mut file = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
    let yaml = indoc! {"
        postgres:
          connection_string: postgres://localhost/db
        osm:
          sources:
            monaco:
              path: ${MONACO}
              tags: [name]
              layers:
                names:
                  attributes: [name]
    "};
    file.write_all(yaml.replace("${MONACO}", monaco).as_bytes()).unwrap();
    let error = Osm::load(file.path()).err().unwrap();
    insta::assert_snapshot!(error, @"`martin test` tests file sources (`osm`, `shapefile`) or database sources, not both; put them in separate configuration files");
}

#[tokio::test]
async fn osm_a_search_misled_by_a_split_string_table_fails_the_review() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/osm/split-string-table.osm.pbf"
    );
    let mut file = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
    write!(
        file,
        indoc! {"
            osm:
              sources:
                split:
                  path: {path}
                  tags: [amenity]
                  layers:
                    cafes:
                      geometry: point
                      where: amenity == 'cafe'
                      attributes: []
        "},
        path = path
    )
    .unwrap();
    let backend = Osm::load(file.path()).unwrap().unwrap();
    let error = review(&backend, 14, None, 100, &Reporter::new()).await.err().unwrap();
    insta::assert_snapshot!(error.to_string().replace(path, "split-string-table.osm.pbf"), @"Cannot read split-string-table.osm.pbf: 2 blocks continue their string table after their first group; please report it at https://github.com/nyurik/osmflow/issues");
}
