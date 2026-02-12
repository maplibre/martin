#![expect(clippy::unreadable_literal)]

use insta::assert_snapshot;
use martin_tile_utils::{MAX_ZOOM, bbox_to_xyz};
use mbtiles::MbtError::InvalidTileIndex;
use mbtiles::{Mbtiles, create_metadata_table};
use rstest::rstest;
use sqlx::{Executor as _, SqliteConnection, query};

async fn new(values: &str) -> (Mbtiles, SqliteConnection) {
    let mbtiles = Mbtiles::new(":memory:").unwrap();
    let mut conn = mbtiles.open().await.unwrap();
    create_metadata_table(&mut conn).await.unwrap();

    conn.execute(
        "CREATE TABLE tiles (
             zoom_level integer,
             tile_column integer,
             tile_row integer,
             tile_data blob,
             PRIMARY KEY(zoom_level, tile_column, tile_row));",
    )
    .await
    .unwrap();

    let sql = format!(
        "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data)
         VALUES ({values});"
    );
    query(&sql).execute(&mut conn).await.expect(&sql);

    (mbtiles, conn)
}

macro_rules! ok {
    ($($vals:tt)*) => {{
        let vals = format!($($vals)*);
        let (mbt, mut conn) = new(&vals).await;
        let res = mbt.check_tiles_type_validity(&mut conn).await;
        assert!(res.is_ok(), "check_tiles_xyz_validity({vals}) = {res:?}, expected Ok");
    }};
}

macro_rules! err {
    ($($vals:tt)*) => {
        let vals = format!($($vals)*);
        let (mbt, mut conn) = new(&vals).await;
        match mbt.check_tiles_type_validity(&mut conn).await {
            Ok(()) => panic!("check_tiles_xyz_validity({vals}) was expected to fail"),
            Err(e) => match e {
                InvalidTileIndex(..) => {}
                _ => panic!("check_tiles_xyz_validity({vals}) = Err({e:?}), expected Err(InvalidTileIndex)"),
            },
        }
    };
}

#[rstest]
#[case("", ", 0, 0, NULL")] // test tile_zoom
#[case("0, ", ", 0, NULL")] // test tile_column
#[case("0, 0, ", ", NULL")] // test tile_row
#[trace]
#[actix_rt::test]
#[tracing_test::traced_test]
async fn integers(#[case] prefix: &str, #[case] suffix: &str) {
    ok!("{prefix} 0 {suffix}");

    err!("{prefix}  NULL {suffix}");
    err!("{prefix}  -1   {suffix}");
    err!("{prefix}  0.2  {suffix}");
    err!("{prefix}  ''   {suffix}");
    err!("{prefix}  'a'  {suffix}");

    err!("{prefix}  CAST(1 AS BLOB)    {suffix}");
    err!("{prefix}  CAST('1' AS BLOB)  {suffix}");

    // These fail for some reason, probably due to internal SQLite casting/affinity rules?
    // err!("{prefix}  '1'  {suffix}");
    // err!("{prefix}  CAST(1 AS REAL)       {suffix}");
    // err!("{prefix}  CAST(1.0 AS NUMERIC)  {suffix}");
    // err!("{prefix}  CAST(1 AS TEXT)       {suffix}");
}

#[rstest]
#[case("", ", 0, NULL")] // test tile_column
#[case("0, ", ", NULL")] // test tile_row
#[trace]
#[actix_rt::test]
#[tracing_test::traced_test]
async fn tile_coordinate(#[case] prefix: &str, #[case] suffix: &str) {
    ok!("0,  {prefix} 0          {suffix}");
    ok!("1,  {prefix} 1          {suffix}");
    ok!("2,  {prefix} 3          {suffix}");
    ok!("3,  {prefix} 7          {suffix}");
    ok!("24, {prefix} 0          {suffix}");
    ok!("24, {prefix} 16777215   {suffix}");
    // ok!("30, {prefix} 0          {suffix}");
    // ok!("30, {prefix} 1073741823 {suffix}");

    err!("0, {prefix} 1 {suffix}");
    err!("1, {prefix} 2 {suffix}");
    err!("2, {prefix} 4 {suffix}");
    err!("3, {prefix} 8 {suffix}");
    err!("24, {prefix} 16777216 {suffix}");
    err!("30, {prefix} 1073741824 {suffix}");
    err!("{MAX_ZOOM}, {prefix} 1073741824 {suffix}");
    err!("{}, {prefix} 0 {suffix}", MAX_ZOOM + 1); // unsupported zoom
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn tile_data() {
    ok!("0, 0, 0, NULL");
    ok!("0, 0, 0, CAST('' AS BLOB)");
    ok!("0, 0, 0, CAST('abc' AS BLOB)");
    ok!("0, 0, 0, CAST(123 AS BLOB)");

    err!("0, 0, 0, 0");
    err!("0, 0, 0, 0.1");
    err!("0, 0, 0, CAST('' AS TEXT)");
    err!("0, 0, 0, CAST('abc' AS TEXT)");
    err!("0, 0, 0, CAST(123 AS TEXT)");
}

#[test]
fn test_box() {
    fn tst(left: f64, bottom: f64, right: f64, top: f64, zoom: u8) -> String {
        let (x0, y0, x1, y1) = bbox_to_xyz(left, bottom, right, top, zoom);
        format!("({x0}, {y0}, {x1}, {y1})")
    }

    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 0), @"(0, 0, 0, 0)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 1), @"(0, 1, 0, 1)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 2), @"(0, 3, 0, 3)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 3), @"(0, 7, 0, 7)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 4), @"(0, 14, 1, 15)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 5), @"(0, 29, 2, 31)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 6), @"(0, 58, 5, 63)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 7), @"(0, 116, 11, 126)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 8), @"(0, 233, 23, 253)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 9), @"(0, 466, 47, 507)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 10), @"(1, 933, 94, 1014)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 11), @"(3, 1866, 188, 2029)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 12), @"(6, 3732, 377, 4059)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 13), @"(12, 7465, 755, 8119)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 14), @"(25, 14931, 1510, 16239)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 15), @"(51, 29863, 3020, 32479)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 16), @"(102, 59727, 6041, 64958)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 17), @"(204, 119455, 12083, 129917)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 18), @"(409, 238911, 24166, 259834)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 19), @"(819, 477823, 48332, 519669)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 20), @"(1638, 955647, 96665, 1039339)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 21), @"(3276, 1911295, 193331, 2078678)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 22), @"(6553, 3822590, 386662, 4157356)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 23), @"(13107, 7645181, 773324, 8314713)");
    assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 24), @"(26214, 15290363, 1546649, 16629427)");

    // All these are incorrect
    // assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 25), @"(33554431, 33554431, 33554431, 33554431)");
    // assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 26), @"(67108863, 67108863, 67108863, 67108863)");
    // assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 27), @"(134217727, 134217727, 134217727, 134217727)");
    // assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 28), @"(268435455, 268435455, 268435455, 268435455)");
    // assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 29), @"(536870911, 536870911, 536870911, 536870911)");
    // assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 30), @"(1073741823, 1073741823, 1073741823, 1073741823)");
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn flat_tables_accept_int_type() {
    use mbtiles::is_flat_tables_type;

    let mbtiles = Mbtiles::new(":memory:").unwrap();
    let mut conn = mbtiles.open().await.unwrap();
    create_metadata_table(&mut conn).await.unwrap();

    conn.execute(
        "CREATE TABLE tiles (
             zoom_level int NOT NULL,
             tile_column int NOT NULL,
             tile_row int NOT NULL,
             tile_data blob,
             PRIMARY KEY(zoom_level, tile_column, tile_row));",
    )
    .await
    .unwrap();

    let result = is_flat_tables_type(&mut conn).await;
    assert!(
        matches!(result, Ok(true)),
        "is_flat_tables_type should accept INT type, got: {result:?}"
    );
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn normalized_tables_accept_int_type() {
    use mbtiles::is_normalized_tables_type;

    let mbtiles = Mbtiles::new(":memory:").unwrap();
    let mut conn = mbtiles.open().await.unwrap();
    create_metadata_table(&mut conn).await.unwrap();

    conn.execute(
        "CREATE TABLE map (
             zoom_level int NOT NULL,
             tile_column int NOT NULL,
             tile_row int NOT NULL,
             tile_id text,
             PRIMARY KEY(zoom_level, tile_column, tile_row));",
    )
    .await
    .unwrap();

    conn.execute(
        "CREATE TABLE images (
             tile_id text NOT NULL PRIMARY KEY,
             tile_data blob);",
    )
    .await
    .unwrap();

    let result = is_normalized_tables_type(&mut conn).await;
    assert!(
        matches!(result, Ok(true)),
        "is_normalized_tables_type should accept INT type, got: {result:?}"
    );
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn int_containing_types_accepted() {
    use mbtiles::{is_flat_tables_type, is_normalized_tables_type};

    // Test flat tables with BIGINT, SMALLINT, TINYINT
    let mbtiles_flat = Mbtiles::new(":memory:").unwrap();
    let mut conn_flat = mbtiles_flat.open().await.unwrap();
    create_metadata_table(&mut conn_flat).await.unwrap();

    conn_flat
        .execute(
            "CREATE TABLE tiles (
                 zoom_level bigint NOT NULL,
                 tile_column smallint NOT NULL,
                 tile_row tinyint NOT NULL,
                 tile_data blob,
                 PRIMARY KEY(zoom_level, tile_column, tile_row));",
        )
        .await
        .unwrap();

    let result_flat = is_flat_tables_type(&mut conn_flat).await;
    assert!(
        matches!(result_flat, Ok(true)),
        "is_flat_tables_type should accept BIGINT/SMALLINT/TINYINT, got: {result_flat:?}"
    );

    // Test normalized tables with BIGINT, SMALLINT, TINYINT
    let mbtiles_norm = Mbtiles::new(":memory:").unwrap();
    let mut conn_norm = mbtiles_norm.open().await.unwrap();
    create_metadata_table(&mut conn_norm).await.unwrap();

    conn_norm
        .execute(
            "CREATE TABLE map (
                 zoom_level bigint NOT NULL,
                 tile_column smallint NOT NULL,
                 tile_row tinyint NOT NULL,
                 tile_id text,
                 PRIMARY KEY(zoom_level, tile_column, tile_row));",
        )
        .await
        .unwrap();

    conn_norm
        .execute(
            "CREATE TABLE images (
                 tile_id text NOT NULL PRIMARY KEY,
                 tile_data blob);",
        )
        .await
        .unwrap();

    let result_norm = is_normalized_tables_type(&mut conn_norm).await;
    assert!(
        matches!(result_norm, Ok(true)),
        "is_normalized_tables_type should accept BIGINT/SMALLINT/TINYINT, got: {result_norm:?}"
    );
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn tiles_with_hash_accepts_int_type() {
    use mbtiles::has_tiles_with_hash;

    let mbtiles = Mbtiles::new(":memory:").unwrap();
    let mut conn = mbtiles.open().await.unwrap();
    create_metadata_table(&mut conn).await.unwrap();

    conn.execute(
        "CREATE TABLE tiles_with_hash (
             zoom_level int NOT NULL,
             tile_column int NOT NULL,
             tile_row int NOT NULL,
             tile_data blob,
             tile_hash text,
             PRIMARY KEY(zoom_level, tile_column, tile_row));",
    )
    .await
    .unwrap();

    let result = has_tiles_with_hash(&mut conn).await;
    assert!(
        matches!(result, Ok(true)),
        "has_tiles_with_hash should accept INT type, got: {result:?}"
    );
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn patch_tables_accept_int_type() {
    use mbtiles::get_patch_type;

    // Test bsdiffraw with INT-containing types
    let mbtiles = Mbtiles::new(":memory:").unwrap();
    let mut conn = mbtiles.open().await.unwrap();
    create_metadata_table(&mut conn).await.unwrap();

    conn.execute(
        "CREATE TABLE bsdiffraw (
             zoom_level bigint NOT NULL,
             tile_column smallint NOT NULL,
             tile_row tinyint NOT NULL,
             patch_data blob,
             tile_xxh3_64_hash int,
             PRIMARY KEY(zoom_level, tile_column, tile_row));",
    )
    .await
    .unwrap();

    let result = get_patch_type(&mut conn).await;
    assert!(
        matches!(result, Ok(Some(_))),
        "get_patch_type should accept INT-containing types, got: {result:?}"
    );
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn flat_without_primary_key_can_be_read() {
    use mbtiles::{Mbtiles, MbtType, MbtError};

    // Create a flat table without PRIMARY KEY
    let mbtiles = Mbtiles::new(":memory:").unwrap();
    let mut conn = mbtiles.open().await.unwrap();
    create_metadata_table(&mut conn).await.unwrap();

    // Create tiles table without PRIMARY KEY (no uniqueness constraint)
    conn.execute(
        "CREATE TABLE tiles (
             zoom_level int NOT NULL,
             tile_column int NOT NULL,
             tile_row int NOT NULL,
             tile_data blob);",
    )
    .await
    .unwrap();

    // Insert a test tile
    conn.execute(
        "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (0, 0, 0, x'00');",
    )
    .await
    .unwrap();

    // detect_type should fail because there's no uniqueness constraint
    let result = mbtiles.detect_type(&mut conn).await;
    assert!(
        matches!(result, Err(MbtError::NoUniquenessConstraint(_))),
        "detect_type should fail without uniqueness constraint, got: {result:?}"
    );

    // But we can still read tiles by explicitly using MbtType::Flat
    let tile_data = mbtiles.get_tile(&mut conn, 0, 0, 0).await.unwrap();
    assert!(
        tile_data.is_some(),
        "Should be able to read tile data even without uniqueness constraint"
    );
    assert_eq!(tile_data.unwrap(), vec![0x00]);

    // And format detection should still work
    use tilejson::TileJSON;
    let tilejson = TileJSON {
        tilejson: "3.0.0".to_string(),
        tiles: vec![],
        ..Default::default()
    };
    let format = mbtiles.detect_format(&tilejson, &mut conn).await;
    assert!(
        format.is_ok(),
        "detect_format should work even without uniqueness constraint"
    );
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn flat_with_primary_key_validates() {
    use mbtiles::{Mbtiles, MbtType};

    // Create a flat table with PRIMARY KEY
    let mbtiles = Mbtiles::new(":memory:").unwrap();
    let mut conn = mbtiles.open().await.unwrap();
    create_metadata_table(&mut conn).await.unwrap();

    // Create tiles table with PRIMARY KEY
    conn.execute(
        "CREATE TABLE tiles (
             zoom_level int NOT NULL,
             tile_column int NOT NULL,
             tile_row int NOT NULL,
             tile_data blob,
             PRIMARY KEY(zoom_level, tile_column, tile_row));",
    )
    .await
    .unwrap();

    // Insert a test tile
    conn.execute(
        "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (0, 0, 0, x'00');",
    )
    .await
    .unwrap();

    // detect_type should succeed
    let result = mbtiles.detect_type(&mut conn).await;
    assert!(
        matches!(result, Ok(MbtType::Flat)),
        "detect_type should succeed with PRIMARY KEY, got: {result:?}"
    );

    // And reading tiles should work
    let tile_data = mbtiles.get_tile(&mut conn, 0, 0, 0).await.unwrap();
    assert!(tile_data.is_some());
    assert_eq!(tile_data.unwrap(), vec![0x00]);
}
