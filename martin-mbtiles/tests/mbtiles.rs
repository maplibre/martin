use std::collections::HashMap;
use std::path::PathBuf;
use std::str::from_utf8;

use ctor::ctor;
use insta::{allow_duplicates, assert_display_snapshot};
use martin_mbtiles::IntegrityCheckType::Off;
use martin_mbtiles::MbtType::{Flat, FlatWithHash, Normalized};
use martin_mbtiles::{apply_diff, create_flat_tables, MbtResult, MbtType, Mbtiles, MbtilesCopier};
use pretty_assertions::assert_eq as pretty_assert_eq;
use rstest::{fixture, rstest};
use serde::Serialize;
use sqlx::{query, query_as, Executor as _, Row, SqliteConnection};

const TILES_V1: &str = "
    INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES
      --(z, x, y, data) -- rules: keep if x=0, edit if x=1, remove if x=2   
        (5, 0, 0, cast('same' as blob))
      , (5, 1, 1, cast('edit-v1' as blob))
      , (5, 2, 2, cast('remove' as blob))
      , (6, 0, 3, cast('same' as blob))
      , (6, 1, 4, cast('edit-v1' as blob))
      , (6, 0, 5, cast('1-keep-1-rm' as blob))
      , (6, 2, 6, cast('1-keep-1-rm' as blob))
      ;";

const TILES_V2: &str = "
    INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES
        (5, 0, 0, cast('same' as blob))        -- no changes
      , (5, 1, 1, cast('edit-v2' as blob))     -- edited in-place
   -- , (5, 2, 2, cast('remove' as blob))      -- this row is deleted
      , (6, 0, 3, cast('same' as blob))        -- no changes, same content as 5/0/0
      , (6, 1, 4, cast('edit-v2a' as blob))    -- edited, used to be same as 5/1/1
      , (6, 0, 5, cast('1-keep-1-rm' as blob)) -- this row is kept (same content as next)
   -- , (6, 2, 6, cast('1-keep-1-rm' as blob)) -- this row is deleted
      , (5, 3, 7, cast('new' as blob))         -- this row is added, dup value
      , (5, 3, 8, cast('new' as blob))         -- this row is added, dup value
      
      -- Expected delta:
      --   5/1/1 edit
      --   5/2/2 remove
      --   5/3/7 add
      --   5/3/8 add
      --   6/1/4 edit
      --   6/2/6 remove
      ;";

const METADATA_V1: &str = "
    INSERT INTO metadata (name, value) VALUES
        ('md-same', 'value - same')
      , ('md-edit', 'value - v1')
      , ('md-remove', 'value - remove')
      ;";

const METADATA_V2: &str = "
    INSERT INTO metadata (name, value) VALUES
        ('md-same', 'value - same')
      , ('md-edit', 'value - v2')
      , ('md-new', 'value - new')
      ;";

#[ctor]
fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

fn path(mbt: &Mbtiles) -> PathBuf {
    PathBuf::from(mbt.filepath())
}

fn copier(src: &Mbtiles, dst: &Mbtiles) -> MbtilesCopier {
    MbtilesCopier::new(path(src), path(dst))
}

fn shorten(v: MbtType) -> &'static str {
    match v {
        Flat => "flat",
        FlatWithHash => "hash",
        Normalized => "norm",
    }
}

async fn open(file: &str) -> MbtResult<(Mbtiles, SqliteConnection)> {
    let mbtiles = Mbtiles::new(file)?;
    let conn = mbtiles.open().await?;
    Ok((mbtiles, conn))
}

macro_rules! open {
    ($function:tt, $($arg:tt)*) => {
        open!(@"", $function, $($arg)*)
    };
    (@$extra:literal, $function:tt, $($arg:tt)*) => {{
        let file = format!("file:{}_{}{}?mode=memory&cache=shared", stringify!($function), format_args!($($arg)*), $extra);
        open(&file).await.unwrap()
    }};
}

/// Create a new SQLite file of given type without agg_tiles_hash metadata value
macro_rules! new_file_no_hash {
    ($function:tt, $dst_type:expr, $sql_meta:expr, $sql_data:expr, $($arg:tt)*) => {{
        new_file!(@true, $function, $dst_type, $sql_meta, $sql_data, $($arg)*)
    }};
}

/// Create a new SQLite file of type $dst_type with the given metadata and tiles
macro_rules! new_file {
    ($function:tt, $dst_type:expr, $sql_meta:expr, $sql_data:expr, $($arg:tt)*) => {
        new_file!(@false, $function, $dst_type, $sql_meta, $sql_data, $($arg)*)
    };

    (@$skip_agg:expr, $function:tt, $dst_type:expr, $sql_meta:expr, $sql_data:expr, $($arg:tt)*) => {{
        let (tmp_mbt, mut cn_tmp) = open!(@"temp", $function, $($arg)*);
        create_flat_tables(&mut cn_tmp).await.unwrap();
        cn_tmp.execute($sql_data).await.unwrap();
        cn_tmp.execute($sql_meta).await.unwrap();

        let (dst_mbt, cn_dst) = open!($function, $($arg)*);
        let mut opt = copier(&tmp_mbt, &dst_mbt);
        opt.dst_type = Some($dst_type);
        opt.skip_agg_tiles_hash = $skip_agg;
        opt.run().await.unwrap();

        (dst_mbt, cn_dst)
    }};
}

macro_rules! assert_snapshot {
    ($actual_value:expr, $($arg:tt)*) => {{
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_suffix(format!($($arg)*));
        let actual_value = &$actual_value;
        settings.bind(|| insta::assert_toml_snapshot!(actual_value));
    }};
}

macro_rules! assert_dump {
    ($connection:expr, $($arg:tt)*) => {{
        let dmp = dump($connection).await.unwrap();
        assert_snapshot!(&dmp, $($arg)*);
        dmp
    }};
}

type Databases = HashMap<(&'static str, MbtType), Vec<SqliteEntry>>;

#[fixture]
#[once]
fn databases() -> Databases {
    futures::executor::block_on(async {
        let mut result = HashMap::new();
        for &typ in &[Flat, FlatWithHash, Normalized] {
            let (raw_mbt, mut cn) =
                new_file_no_hash!(databases, typ, METADATA_V1, TILES_V1, "v1-no-hash-{typ}");
            let dmp = assert_dump!(&mut cn, "v1-no-hash__{typ}");
            result.insert(("v1_no_hash", typ), dmp);

            let (v1_mbt, mut v1_cn) = open!(databases, "v1-{typ}");
            copier(&raw_mbt, &v1_mbt).run().await.unwrap();
            let dmp = assert_dump!(&mut v1_cn, "v1__{typ}");
            let hash = v1_mbt.validate(Off, false).await.unwrap();
            allow_duplicates! {
                assert_display_snapshot!(hash, @"F144D5265985B9D7AC14E7F1F336C6E5");
            }
            result.insert(("v1", typ), dmp);

            let (v2_mbt, mut v2_cn) = new_file!(databases, typ, METADATA_V2, TILES_V2, "v2-{typ}");
            let dmp = assert_dump!(&mut v2_cn, "v2__{typ}");
            let hash = v2_mbt.validate(Off, false).await.unwrap();
            allow_duplicates! {
                assert_display_snapshot!(hash, @"D80BDADB720F2FAD831D3FB0F45408A6");
            }
            result.insert(("v2", typ), dmp);
        }
        result
    })
}

#[rstest]
#[trace]
#[actix_rt::test]
async fn convert(
    #[values(Flat, FlatWithHash, Normalized)] frm_type: MbtType,
    #[values(Flat, FlatWithHash, Normalized)] dst_type: MbtType,
    #[notrace] databases: &Databases,
) -> MbtResult<()> {
    let (frm, to) = (shorten(frm_type), shorten(dst_type));
    let mem = Mbtiles::new(":memory:")?;
    let (frm_mbt, _frm_cn) = new_file!(convert, frm_type, METADATA_V1, TILES_V1, "{frm}-{to}");

    let mut opt = copier(&frm_mbt, &mem);
    opt.dst_type = Some(dst_type);
    let dmp = dump(&mut opt.run().await?).await?;
    pretty_assert_eq!(databases.get(&("v1", dst_type)).unwrap(), &dmp);

    let mut opt = copier(&frm_mbt, &mem);
    opt.dst_type = Some(dst_type);
    opt.zoom_levels.insert(6);
    let z6only = dump(&mut opt.run().await?).await?;
    assert_snapshot!(z6only, "v1__z6__{frm}-{to}");

    let mut opt = copier(&frm_mbt, &mem);
    opt.dst_type = Some(dst_type);
    opt.min_zoom = Some(6);
    pretty_assert_eq!(&z6only, &dump(&mut opt.run().await?).await?);

    let mut opt = copier(&frm_mbt, &mem);
    opt.dst_type = Some(dst_type);
    opt.min_zoom = Some(6);
    opt.max_zoom = Some(6);
    pretty_assert_eq!(&z6only, &dump(&mut opt.run().await?).await?);

    Ok(())
}

#[rstest]
#[trace]
#[actix_rt::test]
async fn diff_apply(
    #[values(Flat, FlatWithHash, Normalized)] v1_type: MbtType,
    #[values(Flat, FlatWithHash, Normalized)] v2_type: MbtType,
    #[values(None, Some(Flat), Some(FlatWithHash), Some(Normalized))] dif_type: Option<MbtType>,
    #[notrace] databases: &Databases,
) -> MbtResult<()> {
    let (v1, v2) = (shorten(v1_type), shorten(v2_type));
    let dif = dif_type.map(shorten).unwrap_or("dflt");

    let (v1_mbt, _v1_cn) =
        new_file! {diff_apply, v1_type, METADATA_V1, TILES_V1, "v1__{v2}-{v1}={dif}"};
    let (v2_mbt, _v2_cn) =
        new_file! {diff_apply, v2_type, METADATA_V2, TILES_V2, "v2__{v2}-{v1}={dif}"};
    let (dif_mbt, _dif_cn) = open!(diff_apply, "dif__{v2}-{v1}={dif}");

    // Compare v1 with v2, and copy anything that's different (i.e. mathematically: v2-v1=diff)
    let mut opt = copier(&v1_mbt, &dif_mbt);
    opt.diff_with_file = Some(path(&v2_mbt));
    if let Some(dif_type) = dif_type {
        opt.dst_type = Some(dif_type);
    }
    assert_dump!(&mut opt.run().await?, "delta__{v2}-{v1}={dif}");

    for target_type in &[Flat, FlatWithHash, Normalized] {
        let trg = shorten(*target_type);
        let expected_v2 = databases.get(&("v2", *target_type)).unwrap();

        let (tar1_mbt, mut tar1_cn) = new_file! {diff_apply, *target_type, METADATA_V1, TILES_V1, "after__{v2}-{v1}={dif}__to__{trg}-v1"};
        apply_diff(path(&tar1_mbt), path(&dif_mbt)).await?;
        let dmp = dump(&mut tar1_cn).await?;
        // pretty_assert_eq!(&dmp, expected_v2);
        if &dmp != expected_v2 {
            assert_snapshot!(dmp, "v2_applied__{v2}-{v1}={dif}__to__{trg}__bad_from_v1");
        }

        let (tar2_mbt, mut tar2_cn) = new_file! {diff_apply, *target_type, METADATA_V2, TILES_V2, "after__{v2}-{v1}={dif}__to__{trg}-v2"};
        apply_diff(path(&tar2_mbt), path(&dif_mbt)).await?;
        let dmp = dump(&mut tar2_cn).await?;
        pretty_assert_eq!(&dmp, expected_v2);
        // if &dmp != expected_v2 {
        //     assert_snapshot!(dmp, "v2_applied__{v2}-{v1}={dif}__to__{trg}__bad_from_v2");
        // }

        // tar2_mbt.validate(Off, false).await.unwrap();
        // if tar2_mbt.validate(Off, false).await.is_err() {
        //     assert_snapshot!(dmp, "v2_applied__{v2}-{v1}={dif}__to__{trg}__validation");
        //     assert_snapshot!(
        //         expected_v2,
        //         "v2_applied__{v2}-{v1}={dif}__to__{trg}__validation_expected"
        //     );
        // }
    }

    Ok(())
}

#[actix_rt::test]
#[ignore]
async fn test_one() {
    let src_type = Flat;
    let dif_type = FlatWithHash;
    let dst_type = Some(Normalized);
    let db = databases();
    diff_apply(src_type, dif_type, dst_type, &db).await.unwrap();
}

#[derive(Debug, sqlx::FromRow, Serialize, PartialEq)]
struct SqliteEntry {
    pub r#type: Option<String>,
    pub tbl_name: Option<String>,
    pub sql: Option<String>,
    #[sqlx(skip)]
    pub values: Option<Vec<String>>,
}

async fn dump(conn: &mut SqliteConnection) -> MbtResult<Vec<SqliteEntry>> {
    let mut result = Vec::new();

    let schema: Vec<SqliteEntry> = query_as(
        "SELECT type, tbl_name, sql
         FROM sqlite_schema
         ORDER BY type != 'table', type, tbl_name",
    )
    .fetch_all(&mut *conn)
    .await?;

    for mut entry in schema {
        let tbl = match (&entry.r#type, &entry.tbl_name) {
            (Some(typ), Some(tbl)) if typ == "table" => tbl,
            _ => {
                result.push(entry);
                continue;
            }
        };

        let sql = format!("PRAGMA table_info({tbl})");
        let columns: Vec<_> = query(&sql)
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .map(|row| {
                let cid: i32 = row.get(0);
                let typ: String = row.get(2);
                (cid as usize, typ)
            })
            .collect();

        let sql = format!("SELECT * FROM {tbl}");
        let rows = query(&sql).fetch_all(&mut *conn).await?;
        let mut values = rows
            .iter()
            .map(|row| {
                let val = columns
                    .iter()
                    .map(|(idx, typ)| {
                        // use sqlx::ValueRef as _;
                        // let raw = row.try_get_raw(*idx).unwrap();
                        // if raw.is_null() {
                        //     return "NULL".to_string();
                        // }
                        // if let Ok(v) = row.try_get::<String, _>(idx) {
                        //     return format!(r#""{v}""#);
                        // }
                        // if let Ok(v) = row.try_get::<Vec<u8>, _>(idx) {
                        //     return format!("blob({})", from_utf8(&v).unwrap());
                        // }
                        // if let Ok(v) = row.try_get::<i32, _>(idx) {
                        //     return v.to_string();
                        // }
                        // if let Ok(v) = row.try_get::<f64, _>(idx) {
                        //     return v.to_string();
                        // }
                        // panic!("Unknown column type: {typ}");
                        (match typ.as_str() {
                            "INTEGER" => row.get::<Option<i32>, _>(idx).map(|v| v.to_string()),
                            "REAL" => row.get::<Option<f64>, _>(idx).map(|v| v.to_string()),
                            "TEXT" => row
                                .get::<Option<String>, _>(idx)
                                .map(|v| format!(r#""{v}""#)),
                            "BLOB" => row
                                .get::<Option<Vec<u8>>, _>(idx)
                                .map(|v| format!("blob({})", from_utf8(&v).unwrap())),
                            _ => panic!("Unknown column type: {typ}"),
                        })
                        .unwrap_or("NULL".to_string())
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("(  {val}  )")
            })
            .collect::<Vec<_>>();

        values.sort();
        entry.values = Some(values);
        result.push(entry);
    }

    Ok(result)
}
