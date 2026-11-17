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
    let monaco = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/osm/monaco.osm.pbf");
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
      input:
        geometry: LINESTRING(7.4135618 43.7315191,7.4137377 43.7315542,7.4137378 43.7315543,7.4137977 43.7316116,7.4137109 43.7317658,7.4136638 43.7318296,7.4136169 43.7318931,7.4134669 43.7319786)
        highway: service
        id: 42271562
        name: null
      output: []
    - source: monaco
      paths:
      - "roads: dropped, `highway` is null in `where`"
      - "roads: dropped, `where` is false"
      input:
        geometry: LINESTRING(7.4207674 43.7389522,7.4214405 43.7396529,7.4222294 43.7404886,7.4229198 43.7413755,7.4259985 43.7450076,7.4271042 43.7461353,7.4296806 43.7483461,7.4311279 43.7494288,7.4332498 43.750677,7.4350394 43.7515102,7.4361839 43.7518194,7.4368175 43.7519639,7.4377083 43.752058,7.4382402 43.7520705,7.4400577 43.7521133,7.4410334 43.7522008,7.4448047 43.7529662,7.4454096 43.753126,7.4460965 43.7533658,7.4466395 43.7536054,7.4480063 43.7543341)
        highway: null
        id: 49782622
        name: Ligne de Marseille à Vintimille
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
        Pass,
    ]
    ");
}

#[test]
fn osm_source_reading_the_tag_id_is_refused() {
    let monaco = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/osm/monaco.osm.pbf");
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
    let monaco = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/osm/monaco.osm.pbf");
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
    let monaco = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/osm/monaco.osm.pbf");
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
    insta::assert_snapshot!(error, @"`martin test` tests OSM sources or database sources, not both; put them in separate configuration files");
}

#[tokio::test]
async fn osm_a_search_misled_by_a_split_string_table_fails_the_review() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/fixtures/osm/split-string-table.osm.pbf"
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
