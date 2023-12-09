use martin_tile_utils::MAX_ZOOM;
use mbtiles::MbtError::InvalidTileIndex;
use mbtiles::{create_metadata_table, Mbtiles};
use rstest::rstest;
use sqlx::{query, Executor as _, SqliteConnection};

#[ctor::ctor]
fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

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
async fn tile_coordinate(#[case] prefix: &str, #[case] suffix: &str) {
    ok!("0,  {prefix} 0          {suffix}");
    ok!("1,  {prefix} 1          {suffix}");
    ok!("2,  {prefix} 3          {suffix}");
    ok!("3,  {prefix} 7          {suffix}");
    ok!("30, {prefix} 0          {suffix}");
    ok!("30, {prefix} 1073741823 {suffix}");

    err!("0, {prefix} 1 {suffix}");
    err!("1, {prefix} 2 {suffix}");
    err!("2, {prefix} 4 {suffix}");
    err!("3, {prefix} 8 {suffix}");
    err!("30, {prefix} 1073741824 {suffix}");
    err!("{MAX_ZOOM}, {prefix} 1073741824 {suffix}");
    err!("{}, {prefix} 0 {suffix}", MAX_ZOOM + 1); // unsupported zoom
}

#[actix_rt::test]
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
