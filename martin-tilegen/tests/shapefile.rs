use std::io::Write as _;

use martin_tilegen::LayerGrid;
use martin_tilegen::plan::{LayerDef, TableDef};
use martin_tilegen::shapefile::{Records, Shapefile, ShapefileSource, ShapefileTable};
use martin_tilegen::source::FeatureSource as _;
use shapefile::dbase::{self, FieldValue, TableWriterBuilder};
use shapefile::{Point, Polygon, PolygonRing, Polyline};
use tilejson::Bounds;

#[test]
fn a_polygon_keeps_its_hole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lakes.shp");
    let mut writer = shapefile::Writer::from_path(&path, TableWriterBuilder::new()).unwrap();
    let lake = Polygon::with_rings(vec![
        PolygonRing::Outer(vec![
            Point::new(0.0, 0.0),
            Point::new(0.0, 10.0),
            Point::new(10.0, 10.0),
            Point::new(10.0, 0.0),
            Point::new(0.0, 0.0),
        ]),
        PolygonRing::Inner(vec![
            Point::new(4.0, 4.0),
            Point::new(6.0, 4.0),
            Point::new(6.0, 6.0),
            Point::new(4.0, 6.0),
            Point::new(4.0, 4.0),
        ]),
    ]);
    writer.write_shape_and_record(&lake, &dbase::Record::default()).unwrap();
    drop(writer);
    std::fs::write(
        dir.path().join("lakes.prj"),
        r#"GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]]"#,
    )
    .unwrap();
    let file = Shapefile::open(&path, dir.path()).unwrap();
    let source = ShapefileSource::new(vec![(
        TableDef {
            columns: file.columns(),
            layers: vec![LayerDef::new(
                "lakes",
                0..=4,
                LayerGrid {
                    extent: 4096,
                    buffer: 64,
                },
            )],
        },
        ShapefileTable {
            file,
            bounds: None,
            records: Records::All,
        },
    )])
    .unwrap();
    let mut read = Vec::new();
    for partition in 0..source.partitions() {
        source
            .read(partition, source.plan().table_keys(), &mut |batch| {
                for feature in batch.features {
                    read.push(format!("{:?} {:?} {:?}", batch.crs, feature.id, feature.geometry));
                }
                Ok(())
            })
            .unwrap();
    }
    insta::assert_snapshot!(read.join("\n"), @"Wgs84 Some(1) Polygon(POLYGON((0.0 0.0,0.0 10.0,10.0 10.0,10.0 0.0,0.0 0.0),(4.0 4.0,6.0 4.0,6.0 6.0,4.0 6.0,4.0 4.0)))");
}

#[test]
fn a_hole_listed_before_its_shell_goes_into_the_smallest_shell_around_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("islands.shp");
    let mut writer = shapefile::Writer::from_path(&path, TableWriterBuilder::new()).unwrap();
    let islands = Polygon::with_rings(vec![
        PolygonRing::Inner(vec![
            Point::new(4.0, 4.0),
            Point::new(5.0, 4.0),
            Point::new(5.0, 5.0),
            Point::new(4.0, 5.0),
            Point::new(4.0, 4.0),
        ]),
        PolygonRing::Outer(vec![
            Point::new(0.0, 0.0),
            Point::new(0.0, 20.0),
            Point::new(20.0, 20.0),
            Point::new(20.0, 0.0),
            Point::new(0.0, 0.0),
        ]),
        PolygonRing::Outer(vec![
            Point::new(3.0, 3.0),
            Point::new(3.0, 6.0),
            Point::new(6.0, 6.0),
            Point::new(6.0, 3.0),
            Point::new(3.0, 3.0),
        ]),
        PolygonRing::Outer(vec![
            Point::new(30.0, 30.0),
            Point::new(30.0, 31.0),
            Point::new(31.0, 31.0),
            Point::new(30.0, 30.0),
        ]),
    ]);
    writer
        .write_shape_and_record(&islands, &dbase::Record::default())
        .unwrap();
    drop(writer);
    std::fs::write(
        dir.path().join("islands.prj"),
        r#"GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]]"#,
    )
    .unwrap();
    let file = Shapefile::open(&path, dir.path()).unwrap();
    let source = ShapefileSource::new(vec![(
        TableDef {
            columns: file.columns(),
            layers: vec![LayerDef::new(
                "islands",
                0..=4,
                LayerGrid {
                    extent: 4096,
                    buffer: 64,
                },
            )],
        },
        ShapefileTable {
            file,
            bounds: None,
            records: Records::All,
        },
    )])
    .unwrap();
    let mut read = Vec::new();
    for partition in 0..source.partitions() {
        source
            .read(partition, source.plan().table_keys(), &mut |batch| {
                for feature in batch.features {
                    read.push(format!("{:?}", feature.geometry));
                }
                Ok(())
            })
            .unwrap();
    }
    insta::assert_snapshot!(read.join("\n"), @"Polygons([POLYGON((0.0 0.0,0.0 20.0,20.0 20.0,20.0 0.0,0.0 0.0)), POLYGON((3.0 3.0,3.0 6.0,6.0 6.0,6.0 3.0,3.0 3.0),(4.0 4.0,5.0 4.0,5.0 5.0,4.0 5.0,4.0 4.0)), POLYGON((30.0 30.0,30.0 31.0,31.0 31.0,30.0 30.0))])");
}

#[test]
fn a_hole_outside_every_shell_is_a_polygon_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("islands.shp");
    let mut writer = shapefile::Writer::from_path(&path, TableWriterBuilder::new()).unwrap();
    let islands = Polygon::with_rings(vec![
        PolygonRing::Outer(vec![
            Point::new(0.0, 0.0),
            Point::new(0.0, 1.0),
            Point::new(1.0, 1.0),
            Point::new(0.0, 0.0),
        ]),
        PolygonRing::Outer(vec![
            Point::new(10.0, 10.0),
            Point::new(10.0, 11.0),
            Point::new(11.0, 11.0),
            Point::new(10.0, 10.0),
        ]),
        PolygonRing::Inner(vec![
            Point::new(20.0, 20.0),
            Point::new(21.0, 20.0),
            Point::new(21.0, 21.0),
            Point::new(20.0, 20.0),
        ]),
    ]);
    writer
        .write_shape_and_record(&islands, &dbase::Record::default())
        .unwrap();
    drop(writer);
    std::fs::write(
        dir.path().join("islands.prj"),
        r#"GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]]"#,
    )
    .unwrap();
    let file = Shapefile::open(&path, dir.path()).unwrap();
    let source = ShapefileSource::new(vec![(
        TableDef {
            columns: file.columns(),
            layers: vec![LayerDef::new(
                "islands",
                0..=4,
                LayerGrid {
                    extent: 4096,
                    buffer: 64,
                },
            )],
        },
        ShapefileTable {
            file,
            bounds: None,
            records: Records::All,
        },
    )])
    .unwrap();
    let mut read = Vec::new();
    for partition in 0..source.partitions() {
        source
            .read(partition, source.plan().table_keys(), &mut |batch| {
                for feature in batch.features {
                    read.push(format!("{:?}", feature.geometry));
                }
                Ok(())
            })
            .unwrap();
    }
    insta::assert_snapshot!(read.join("\n"), @"Polygons([POLYGON((0.0 0.0,0.0 1.0,1.0 1.0,0.0 0.0)), POLYGON((10.0 10.0,10.0 11.0,11.0 11.0,10.0 10.0)), POLYGON((20.0 20.0,21.0 20.0,21.0 21.0,20.0 20.0))])");
}

#[test]
fn a_polygon_of_several_parts_is_a_multipolygon_and_a_line_of_several_parts_a_multiline() {
    let dir = tempfile::tempdir().unwrap();
    let polygons = dir.path().join("parks.shp");
    let mut writer = shapefile::Writer::from_path(&polygons, TableWriterBuilder::new()).unwrap();
    let parks = Polygon::with_rings(vec![
        PolygonRing::Outer(vec![
            Point::new(0.0, 0.0),
            Point::new(0.0, 1.0),
            Point::new(1.0, 1.0),
            Point::new(0.0, 0.0),
        ]),
        PolygonRing::Outer(vec![
            Point::new(5.0, 5.0),
            Point::new(5.0, 6.0),
            Point::new(6.0, 6.0),
            Point::new(5.0, 5.0),
        ]),
    ]);
    writer
        .write_shape_and_record(&parks, &dbase::Record::default())
        .unwrap();
    drop(writer);
    let lines = dir.path().join("rivers.shp");
    let mut writer = shapefile::Writer::from_path(&lines, TableWriterBuilder::new()).unwrap();
    let rivers = Polyline::with_parts(vec![
        vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)],
        vec![Point::new(2.0, 2.0), Point::new(3.0, 2.0)],
    ]);
    writer
        .write_shape_and_record(&rivers, &dbase::Record::default())
        .unwrap();
    drop(writer);
    for name in ["parks.prj", "rivers.prj"] {
        std::fs::write(
            dir.path().join(name),
            r#"GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]]"#,
        )
        .unwrap();
    }
    let grid = LayerGrid {
        extent: 4096,
        buffer: 64,
    };
    let parks = Shapefile::open(&polygons, dir.path()).unwrap();
    let rivers = Shapefile::open(&lines, dir.path()).unwrap();
    let source = ShapefileSource::new(vec![
        (
            TableDef {
                columns: parks.columns(),
                layers: vec![LayerDef::new("parks", 0..=4, grid)],
            },
            ShapefileTable {
                file: parks,
                bounds: None,
                records: Records::All,
            },
        ),
        (
            TableDef {
                columns: rivers.columns(),
                layers: vec![LayerDef::new("rivers", 0..=4, grid)],
            },
            ShapefileTable {
                file: rivers,
                bounds: None,
                records: Records::All,
            },
        ),
    ])
    .unwrap();
    let mut read = Vec::new();
    for partition in 0..source.partitions() {
        source
            .read(partition, source.plan().table_keys(), &mut |batch| {
                for feature in batch.features {
                    read.push(format!("table {} {:?}", batch.table, feature.geometry));
                }
                Ok(())
            })
            .unwrap();
    }
    insta::assert_snapshot!(read.join("\n"), @"
    table 0 Polygons([POLYGON((0.0 0.0,0.0 1.0,1.0 1.0,0.0 0.0)), POLYGON((5.0 5.0,5.0 6.0,6.0 6.0,5.0 5.0))])
    table 1 Lines([LINESTRING(0.0 0.0,1.0 1.0), LINESTRING(2.0 2.0,3.0 2.0)])
    ");
}

#[test]
fn a_shapefile_in_web_mercator_is_read_in_web_mercator() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("water_polygons.shp");
    let mut writer = shapefile::Writer::from_path(&path, TableWriterBuilder::new()).unwrap();
    writer
        .write_shape_and_record(&Point::new(1_000_000.0, 2_000_000.0), &dbase::Record::default())
        .unwrap();
    drop(writer);
    std::fs::write(
        dir.path().join("water_polygons.prj"),
        r#"PROJCS["WGS_1984_Web_Mercator_Auxiliary_Sphere",GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Mercator_Auxiliary_Sphere"],PARAMETER["False_Easting",0.0],PARAMETER["False_Northing",0.0],PARAMETER["Central_Meridian",0.0],PARAMETER["Standard_Parallel_1",0.0],PARAMETER["Auxiliary_Sphere_Type",0.0],UNIT["Meter",1.0]]"#,
    )
    .unwrap();
    let file = Shapefile::open(&path, dir.path()).unwrap();
    let source = ShapefileSource::new(vec![(
        TableDef {
            columns: file.columns(),
            layers: vec![LayerDef::new(
                "ocean",
                0..=4,
                LayerGrid {
                    extent: 4096,
                    buffer: 64,
                },
            )],
        },
        ShapefileTable {
            file,
            bounds: None,
            records: Records::All,
        },
    )])
    .unwrap();
    let mut read = Vec::new();
    for partition in 0..source.partitions() {
        source
            .read(partition, source.plan().table_keys(), &mut |batch| {
                for feature in batch.features {
                    read.push(format!("{:?} {:?}", batch.crs, feature.geometry));
                }
                Ok(())
            })
            .unwrap();
    }
    insta::assert_snapshot!(read.join("\n"), @"WebMercator Point(COORD(1000000.0 2000000.0))");
}

#[test]
fn a_shapefile_in_gdal_web_mercator_is_read_in_web_mercator() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("water_polygons.shp");
    let mut writer = shapefile::Writer::from_path(&path, TableWriterBuilder::new()).unwrap();
    writer
        .write_shape_and_record(&Point::new(1_000_000.0, 2_000_000.0), &dbase::Record::default())
        .unwrap();
    drop(writer);
    std::fs::write(
        dir.path().join("water_polygons.prj"),
        r#"PROJCS["WGS 84 / Pseudo-Mercator",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AUTHORITY["EPSG","4326"]],PROJECTION["Mercator_1SP"],PARAMETER["central_meridian",0],PARAMETER["scale_factor",1],PARAMETER["false_easting",0],PARAMETER["false_northing",0],UNIT["metre",1,AUTHORITY["EPSG","9001"]],AXIS["Easting",EAST],AXIS["Northing",NORTH],EXTENSION["PROJ4","+proj=merc +a=6378137 +b=6378137 +lat_ts=0 +lon_0=0 +x_0=0 +y_0=0 +k=1 +units=m +nadgrids=@null +wktext +no_defs"],AUTHORITY["EPSG","3857"]]"#,
    )
    .unwrap();
    let file = Shapefile::open(&path, dir.path()).unwrap();
    insta::assert_debug_snapshot!(file.crs(), @"WebMercator");
}

#[test]
fn a_shapefile_in_another_projection_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("roads.shp");
    let mut writer = shapefile::Writer::from_path(&path, TableWriterBuilder::new()).unwrap();
    writer
        .write_shape_and_record(&Point::new(500_000.0, 5_800_000.0), &dbase::Record::default())
        .unwrap();
    drop(writer);
    std::fs::write(
        dir.path().join("roads.prj"),
        r#"PROJCS["ETRS_1989_UTM_Zone_32N",GEOGCS["GCS_ETRS_1989",DATUM["D_ETRS_1989",SPHEROID["GRS_1980",6378137.0,298.257222101]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Transverse_Mercator"],PARAMETER["False_Easting",500000.0],PARAMETER["False_Northing",0.0],PARAMETER["Central_Meridian",9.0],PARAMETER["Scale_Factor",0.9996],PARAMETER["Latitude_Of_Origin",0.0],UNIT["Meter",1.0]]"#,
    )
    .unwrap();
    let err = Shapefile::open(&path, dir.path()).unwrap_err();
    insta::assert_snapshot!(err.to_string().replace(&dir.path().display().to_string(), "<dir>"), @"<dir>/roads.shp is in `ETRS_1989_UTM_Zone_32N`; Martin reads shapefiles in EPSG:4326 or EPSG:3857 only");
}

#[test]
fn a_shapefile_in_ellipsoidal_mercator_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("roads.shp");
    let mut writer = shapefile::Writer::from_path(&path, TableWriterBuilder::new()).unwrap();
    writer
        .write_shape_and_record(&Point::new(1_000_000.0, 2_000_000.0), &dbase::Record::default())
        .unwrap();
    drop(writer);
    std::fs::write(
        dir.path().join("roads.prj"),
        r#"PROJCS["WGS_1984_World_Mercator",GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Mercator"],PARAMETER["False_Easting",0.0],PARAMETER["False_Northing",0.0],PARAMETER["Central_Meridian",0.0],PARAMETER["Standard_Parallel_1",0.0],UNIT["Meter",1.0]]"#,
    )
    .unwrap();
    let err = Shapefile::open(&path, dir.path()).unwrap_err();
    insta::assert_snapshot!(err.to_string().replace(&dir.path().display().to_string(), "<dir>"), @"<dir>/roads.shp is in `WGS_1984_World_Mercator`; Martin reads shapefiles in EPSG:4326 or EPSG:3857 only");
}

#[test]
fn a_shapefile_without_a_prj_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("roads.shp");
    let mut writer = shapefile::Writer::from_path(&path, TableWriterBuilder::new()).unwrap();
    writer
        .write_shape_and_record(&Point::new(1.0, 2.0), &dbase::Record::default())
        .unwrap();
    drop(writer);
    let err = Shapefile::open(&path, dir.path()).unwrap_err();
    insta::assert_snapshot!(err.to_string().replace(&dir.path().display().to_string(), "<dir>"), @"<dir>/roads.shp has no .prj file; Martin needs it to tell EPSG:4326 from EPSG:3857");
}

#[test]
fn fields_become_properties_of_the_kind_of_their_type() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("places.shp");
    let fields = TableWriterBuilder::new()
        .add_character_field("name".try_into().unwrap(), 20)
        .add_numeric_field("population".try_into().unwrap(), 10, 0)
        .add_numeric_field("area".try_into().unwrap(), 10, 2)
        .add_logical_field("capital".try_into().unwrap())
        .add_date_field("founded".try_into().unwrap());
    let mut writer = shapefile::Writer::from_path(&path, fields).unwrap();
    let mut berlin = dbase::Record::default();
    berlin.insert("name".to_owned(), FieldValue::Character(Some("Berlin".to_owned())));
    berlin.insert("population".to_owned(), FieldValue::Numeric(Some(3_850_000.0)));
    berlin.insert("area".to_owned(), FieldValue::Numeric(Some(891.12)));
    berlin.insert("capital".to_owned(), FieldValue::Logical(Some(true)));
    berlin.insert(
        "founded".to_owned(),
        FieldValue::Date(Some(dbase::Date::new(28, 10, 1237).unwrap())),
    );
    writer.write_shape_and_record(&Point::new(13.4, 52.5), &berlin).unwrap();
    let mut unnamed = dbase::Record::default();
    unnamed.insert("name".to_owned(), FieldValue::Character(None));
    unnamed.insert("population".to_owned(), FieldValue::Numeric(None));
    unnamed.insert("area".to_owned(), FieldValue::Numeric(Some(2.0)));
    unnamed.insert("capital".to_owned(), FieldValue::Logical(None));
    unnamed.insert("founded".to_owned(), FieldValue::Date(None));
    writer.write_shape_and_record(&Point::new(1.0, 1.0), &unnamed).unwrap();
    drop(writer);
    std::fs::write(
        dir.path().join("places.prj"),
        r#"GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]]"#,
    )
    .unwrap();
    let file = Shapefile::open(&path, dir.path()).unwrap();
    let columns = file.columns();
    let source = ShapefileSource::new(vec![(
        TableDef {
            columns: columns.clone(),
            layers: vec![LayerDef::new(
                "places",
                0..=4,
                LayerGrid {
                    extent: 4096,
                    buffer: 64,
                },
            )],
        },
        ShapefileTable {
            file,
            bounds: None,
            records: Records::All,
        },
    )])
    .unwrap();
    let mut read = Vec::new();
    for partition in 0..source.partitions() {
        source
            .read(partition, source.plan().table_keys(), &mut |batch| {
                for feature in batch.features {
                    read.push(format!("{:?} {:?}", feature.id, feature.props));
                }
                Ok(())
            })
            .unwrap();
    }
    insta::assert_snapshot!(format!("{columns:?}\n{}", read.join("\n")), @r#"
    [Value("name", Some(String)), Value("population", Some(Int)), Value("area", Some(Float)), Value("capital", Some(Bool)), Value("founded", Some(String))]
    Some(1) [(KeyId(0), Str("Berlin")), (KeyId(1), I64(3850000)), (KeyId(2), F64(891.12)), (KeyId(3), Bool(true)), (KeyId(4), Str("1237-10-28"))]
    Some(2) [(KeyId(2), F64(2.0))]
    "#);
}

#[test]
fn a_zip_file_reads_like_the_shapefile_it_holds() {
    let dir = tempfile::tempdir().unwrap();
    let parts = dir.path().join("parts");
    std::fs::create_dir_all(&parts).unwrap();
    let path = parts.join("water_polygons.shp");
    let fields = TableWriterBuilder::new().add_character_field("kind".try_into().unwrap(), 10);
    let mut writer = shapefile::Writer::from_path(&path, fields).unwrap();
    let mut record = dbase::Record::default();
    record.insert("kind".to_owned(), FieldValue::Character(Some("sea".to_owned())));
    let sea = Polygon::new(PolygonRing::Outer(vec![
        Point::new(0.0, 0.0),
        Point::new(0.0, 1000.0),
        Point::new(1000.0, 1000.0),
        Point::new(0.0, 0.0),
    ]));
    writer.write_shape_and_record(&sea, &record).unwrap();
    drop(writer);
    std::fs::write(
        parts.join("water_polygons.prj"),
        r#"PROJCS["WGS_1984_Web_Mercator_Auxiliary_Sphere",GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Mercator_Auxiliary_Sphere"],PARAMETER["False_Easting",0.0],PARAMETER["False_Northing",0.0],PARAMETER["Central_Meridian",0.0],PARAMETER["Standard_Parallel_1",0.0],PARAMETER["Auxiliary_Sphere_Type",0.0],UNIT["Meter",1.0]]"#,
    )
    .unwrap();
    std::fs::write(parts.join("water_polygons.cpg"), "UTF-8\n").unwrap();
    let zipped = dir.path().join("water-polygons-split-3857.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&zipped).unwrap());
    zip.add_directory("water-polygons-split-3857/", zip::write::SimpleFileOptions::default())
        .unwrap();
    for ext in ["shp", "shx", "dbf", "prj", "cpg"] {
        zip.start_file(
            format!("water-polygons-split-3857/water_polygons.{ext}"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(&std::fs::read(parts.join(format!("water_polygons.{ext}"))).unwrap())
            .unwrap();
    }
    zip.finish().unwrap();
    let unpacked = dir.path().join("temp");
    std::fs::create_dir_all(&unpacked).unwrap();
    let file = Shapefile::open(&zipped, &unpacked).unwrap();
    let source = ShapefileSource::new(vec![(
        TableDef {
            columns: file.columns(),
            layers: vec![LayerDef::new(
                "ocean",
                0..=4,
                LayerGrid {
                    extent: 4096,
                    buffer: 64,
                },
            )],
        },
        ShapefileTable {
            file,
            bounds: None,
            records: Records::All,
        },
    )])
    .unwrap();
    let mut read = Vec::new();
    for partition in 0..source.partitions() {
        source
            .read(partition, source.plan().table_keys(), &mut |batch| {
                for feature in batch.features {
                    read.push(format!(
                        "{:?} {:?} {:?} {:?}",
                        batch.crs, feature.id, feature.props, feature.geometry
                    ));
                }
                Ok(())
            })
            .unwrap();
    }
    insta::assert_snapshot!(read.join("\n"), @r#"WebMercator Some(1) [(KeyId(0), Str("sea"))] Polygon(POLYGON((0.0 0.0,0.0 1000.0,1000.0 1000.0,0.0 0.0)))"#);
    assert_eq!(std::fs::read_dir(&unpacked).unwrap().count(), 1);
    drop(source);
    assert_eq!(std::fs::read_dir(&unpacked).unwrap().count(), 0);
}

#[test]
fn a_zip_file_with_two_shapefiles_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let zipped = dir.path().join("two.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&zipped).unwrap());
    for name in ["land.shp", "water.shp"] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"").unwrap();
    }
    zip.finish().unwrap();
    let err = Shapefile::open(&zipped, dir.path()).unwrap_err();
    insta::assert_snapshot!(err.to_string().replace(&dir.path().display().to_string(), "<dir>"), @"<dir>/two.zip holds 2 shapefiles; Martin reads a zip file that holds exactly one");
}

#[test]
fn one_record_is_read_by_its_number() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("places.shp");
    let mut writer = shapefile::Writer::from_path(&path, TableWriterBuilder::new()).unwrap();
    for x in [1.0, 2.0, 3.0] {
        writer
            .write_shape_and_record(&Point::new(x, x), &dbase::Record::default())
            .unwrap();
    }
    drop(writer);
    std::fs::write(
        dir.path().join("places.prj"),
        r#"GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]]"#,
    )
    .unwrap();
    let file = Shapefile::open(&path, dir.path()).unwrap();
    let source = ShapefileSource::new(vec![(
        TableDef {
            columns: file.columns(),
            layers: vec![LayerDef::new(
                "places",
                0..=4,
                LayerGrid {
                    extent: 4096,
                    buffer: 64,
                },
            )],
        },
        ShapefileTable {
            file,
            bounds: None,
            records: Records::One(2),
        },
    )])
    .unwrap();
    let mut read = Vec::new();
    for partition in 0..source.partitions() {
        source
            .read(partition, source.plan().table_keys(), &mut |batch| {
                for feature in batch.features {
                    read.push(format!("{:?} {:?}", feature.id, feature.geometry));
                }
                Ok(())
            })
            .unwrap();
    }
    insta::assert_snapshot!(read.join("\n"), @"Some(2) Point(COORD(2.0 2.0))");
}

#[test]
fn records_outside_the_bounds_are_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("places.shp");
    let mut writer = shapefile::Writer::from_path(&path, TableWriterBuilder::new()).unwrap();
    for (x, y) in [(1_000_000.0, 1_000_000.0), (-1_000_000.0, 1_000_000.0)] {
        writer
            .write_shape_and_record(&Point::new(x, y), &dbase::Record::default())
            .unwrap();
    }
    drop(writer);
    std::fs::write(
        dir.path().join("places.prj"),
        r#"PROJCS["WGS_1984_Web_Mercator_Auxiliary_Sphere",GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Mercator_Auxiliary_Sphere"],PARAMETER["False_Easting",0.0],PARAMETER["False_Northing",0.0],PARAMETER["Central_Meridian",0.0],PARAMETER["Standard_Parallel_1",0.0],PARAMETER["Auxiliary_Sphere_Type",0.0],UNIT["Meter",1.0]]"#,
    )
    .unwrap();
    let file = Shapefile::open(&path, dir.path()).unwrap();
    let source = ShapefileSource::new(vec![(
        TableDef {
            columns: file.columns(),
            layers: vec![LayerDef::new(
                "places",
                0..=4,
                LayerGrid {
                    extent: 4096,
                    buffer: 64,
                },
            )],
        },
        ShapefileTable {
            file,
            bounds: Some(Bounds::new(0.0, 0.0, 20.0, 20.0)),
            records: Records::All,
        },
    )])
    .unwrap();
    let mut read = Vec::new();
    for partition in 0..source.partitions() {
        source
            .read(partition, source.plan().table_keys(), &mut |batch| {
                for feature in batch.features {
                    read.push(format!("{:?} {:?}", feature.id, feature.geometry));
                }
                Ok(())
            })
            .unwrap();
    }
    insta::assert_snapshot!(read.join("\n"), @"Some(1) Point(COORD(1000000.0 1000000.0))");
}
