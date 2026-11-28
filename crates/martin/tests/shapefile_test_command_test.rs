#![cfg(all(feature = "osm", feature = "shapefile"))]

use std::io::Write as _;

use indoc::formatdoc;
use martin::generate::spec::backend::TestBackend as _;
use martin::generate::spec::file::yaml;
use martin::generate::spec::files::Files;
use martin::generate::spec::report::Reporter;
use martin::generate::spec::shapefile::Shapefiles;
use martin::generate::spec::{check, review};

#[tokio::test]
async fn one_spec_has_cases_of_shapefile_records_and_osm_objects() {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures");
    let mut config = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
    config
        .write_all(
            formatdoc! {"
                shapefile:
                  sources:
                    ocean:
                      path: {fixtures}/shapefile/ocean.zip
                      layers:
                        ocean:
                          geometry: polygon
                          attributes: []
                osm:
                  sources:
                    monaco:
                      path: {fixtures}/osm/monaco.osm.pbf
                      tags: [amenity, name]
                      layers:
                        cafes:
                          geometry: point
                          where: amenity == 'cafe'
                          attributes: [name]
            "}
            .as_bytes(),
        )
        .unwrap();
    let backend = Files::load(config.path()).unwrap().unwrap();
    let mut review = review(&backend, 2, None, 10, &Reporter::new()).await.unwrap();
    review.accept_all();
    insta::assert_snapshot!(yaml(&review.spec()), @r#"
    max_zoom: 2
    cases:
    - source: ocean
      paths:
      - "ocean: every feature"
      - "ocean: crosses a z2 tile edge"
      input:
        geometry: POLYGON((-3000000 0,-3000000 1252344.2714243277,-1747655.7285756723 1252344.2714243277,-1747655.7285756723 0,-3000000 0))
        record: 1
      output:
      - layer: ocean
        id: 1
        geometry: polygon
        zooms: 0-2
    - source: monaco
      paths:
      - "cafes: kept by `where`"
      input:
        amenity: cafe
        geometry: POINT(7.4216664 43.7311665)
        id: 9179787631
        name: Snack Bar La Pampa
      output:
      - layer: cafes
        id: 9179787631
        geometry: point
        zooms: 0-2
        attributes:
          name: Snack Bar La Pampa
    - source: monaco
      paths:
      - "cafes: dropped, `where` is false"
      - "cafes: dropped, `amenity` is null in `where`"
      input:
        amenity: null
        geometry: POINT(7.4234985 43.7372399)
        id: 219118861
        name: null
      output: []
    "#);
}

#[tokio::test]
async fn cases_of_a_shapefile_spec_pass() {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures");
    let mut config = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
    config
        .write_all(
            formatdoc! {"
                shapefile:
                  sources:
                    ocean:
                      path: {fixtures}/shapefile/ocean.zip
                      layers:
                        ocean:
                          geometry: polygon
                          where: x == 1
                          attributes: [y]
            "}
            .as_bytes(),
        )
        .unwrap();
    let backend = Shapefiles::load(config.path()).unwrap().unwrap();
    let mut review = review(&backend, 3, None, 10, &Reporter::new()).await.unwrap();
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
    ]
    ");
}

#[tokio::test]
async fn a_case_of_a_shapefile_renders_only_its_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("places.shp");
    let fields = shapefile::dbase::TableWriterBuilder::new().add_character_field("name".try_into().unwrap(), 10);
    let mut writer = shapefile::Writer::from_path(&path, fields).unwrap();
    for (name, x) in [("first", 1.0), ("second", 2.0)] {
        let mut record = shapefile::dbase::Record::default();
        record.insert(
            "name".to_owned(),
            shapefile::dbase::FieldValue::Character(Some(name.to_owned())),
        );
        writer
            .write_shape_and_record(&shapefile::Point::new(x, x), &record)
            .unwrap();
    }
    drop(writer);
    std::fs::write(
        dir.path().join("places.prj"),
        r#"GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]]"#,
    )
    .unwrap();
    let config = dir.path().join("config.yaml");
    std::fs::write(
        &config,
        formatdoc! {"
            shapefile:
              sources:
                places:
                  path: {path}
                  layers:
                    places:
                      attributes: [name]
        ", path = path.display()},
    )
    .unwrap();
    let backend = Shapefiles::load(&config).unwrap().unwrap();
    let mut review = review(&backend, 0, None, 10, &Reporter::new()).await.unwrap();
    review.accept_all();
    insta::assert_snapshot!(yaml(&review.spec()), @r#"
    max_zoom: 0
    cases:
    - source: places
      paths:
      - "places: every feature"
      input:
        geometry: POINT(1 1)
        name: first
        record: 1
      output:
      - layer: places
        id: 1
        geometry: point
        zooms: 0-0
        attributes:
          name: first
    "#);
}

#[test]
fn a_shapefile_with_a_field_named_record_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("places.shp");
    let fields = shapefile::dbase::TableWriterBuilder::new().add_numeric_field("record".try_into().unwrap(), 10, 0);
    let mut writer = shapefile::Writer::from_path(&path, fields).unwrap();
    let mut record = shapefile::dbase::Record::default();
    record.insert("record".to_owned(), shapefile::dbase::FieldValue::Numeric(Some(7.0)));
    writer
        .write_shape_and_record(&shapefile::Point::new(1.0, 2.0), &record)
        .unwrap();
    drop(writer);
    std::fs::write(
        dir.path().join("places.prj"),
        r#"GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]]"#,
    )
    .unwrap();
    let config = dir.path().join("config.yaml");
    std::fs::write(
        &config,
        formatdoc! {"
            shapefile:
              sources:
                places:
                  path: {path}
                  layers:
                    places: {{}}
        ", path = path.display()},
    )
    .unwrap();
    let error = Shapefiles::load(&config).err().unwrap();
    insta::assert_snapshot!(error, @"Source `places` has a field `record`, which `martin test` reserves in each case's input");
}

#[test]
fn a_shapefile_with_a_field_named_geometry_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("places.shp");
    let fields = shapefile::dbase::TableWriterBuilder::new().add_character_field("geometry".try_into().unwrap(), 10);
    let mut writer = shapefile::Writer::from_path(&path, fields).unwrap();
    let mut record = shapefile::dbase::Record::default();
    record.insert(
        "geometry".to_owned(),
        shapefile::dbase::FieldValue::Character(Some("x".to_owned())),
    );
    writer
        .write_shape_and_record(&shapefile::Point::new(1.0, 2.0), &record)
        .unwrap();
    drop(writer);
    std::fs::write(
        dir.path().join("places.prj"),
        r#"GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]]"#,
    )
    .unwrap();
    let config = dir.path().join("config.yaml");
    std::fs::write(
        &config,
        formatdoc! {"
            shapefile:
              sources:
                places:
                  path: {path}
                  layers:
                    places: {{}}
        ", path = path.display()},
    )
    .unwrap();
    let error = Shapefiles::load(&config).err().unwrap();
    insta::assert_snapshot!(error, @"Source `places` has a field `geometry`, which `martin test` reserves in each case's input");
}

#[test]
fn a_source_id_in_shapefile_and_osm_is_refused() {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures");
    let mut config = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
    config
        .write_all(
            formatdoc! {"
                shapefile:
                  sources:
                    water:
                      path: {fixtures}/shapefile/ocean.zip
                      layers:
                        ocean: {{}}
                osm:
                  sources:
                    water:
                      path: {fixtures}/osm/monaco.osm.pbf
                      tags: [natural]
                      layers:
                        lakes: {{}}
            "}
            .as_bytes(),
        )
        .unwrap();
    let error = Files::load(config.path()).err().unwrap();
    insta::assert_snapshot!(error, @"Source id `water` is both a `shapefile` and an `osm` source; give them different ids");
}
