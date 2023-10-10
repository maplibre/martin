use std::collections::HashMap;
use std::path::PathBuf;
use std::str::from_utf8;

use ctor::ctor;
use insta::{allow_duplicates, assert_display_snapshot};
use log::info;
use martin_mbtiles::IntegrityCheckType::Off;
use martin_mbtiles::MbtType::{Flat, FlatWithHash, Normalized};
use martin_mbtiles::{apply_patch, create_flat_tables, MbtResult, MbtType, Mbtiles, MbtilesCopier};
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
        for &mbt_typ in &[Flat, FlatWithHash, Normalized] {
            let typ = shorten(mbt_typ);
            let (raw_mbt, mut cn) = new_file_no_hash!(
                databases,
                mbt_typ,
                METADATA_V1,
                TILES_V1,
                "{typ}__v1-no-hash"
            );
            let dmp = assert_dump!(&mut cn, "{typ}__v1-no-hash");
            result.insert(("v1_no_hash", mbt_typ), dmp);

            let (v1_mbt, mut v1_cn) = open!(databases, "{typ}__v1");
            copier(&raw_mbt, &v1_mbt).run().await.unwrap();
            let dmp = assert_dump!(&mut v1_cn, "{typ}__v1");
            let hash = v1_mbt.validate(Off, false).await.unwrap();
            allow_duplicates! {
                assert_display_snapshot!(hash, @"0063DADF9C78A376418DB0D2B00A5F80");
            }
            result.insert(("v1", mbt_typ), dmp);

            let (v2_mbt, mut v2_cn) =
                new_file!(databases, mbt_typ, METADATA_V2, TILES_V2, "{typ}__v2");
            let dmp = assert_dump!(&mut v2_cn, "{typ}__v2");
            let hash = v2_mbt.validate(Off, false).await.unwrap();
            allow_duplicates! {
                assert_display_snapshot!(hash, @"5C90855D70120501451BDD08CA71341A");
            }
            result.insert(("v2", mbt_typ), dmp);

            let (dif_mbt, mut dif_cn) = open!(databases, "{typ}__dif");
            let mut opt = copier(&v1_mbt, &dif_mbt);
            opt.diff_with_file = Some(path(&v2_mbt));
            opt.run().await.unwrap();
            let dmp = assert_dump!(&mut dif_cn, "{typ}__dif");
            // let hash = dif_mbt.validate(Off, false).await.unwrap();
            // allow_duplicates! {
            //     assert_display_snapshot!(hash, @"AB9EE21538C1D28BB357ABB3A45BD6BD");
            // }
            result.insert(("dif", mbt_typ), dmp);
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
    let prefix = format!("{v2}-{v1}={dif}");

    let (v1_mbt, _v1_cn) = new_file! {diff_apply, v1_type, METADATA_V1, TILES_V1, "{prefix}__v1"};
    let (v2_mbt, _v2_cn) = new_file! {diff_apply, v2_type, METADATA_V2, TILES_V2, "{prefix}__v2"};
    let (dif_mbt, _dif_cn) = open!(diff_apply, "{prefix}__dif");

    info!("TEST: Compare v1 with v2, and copy anything that's different (i.e. mathematically: v2-v1=diff)");
    let mut opt = copier(&v1_mbt, &dif_mbt);
    opt.diff_with_file = Some(path(&v2_mbt));
    if let Some(dif_type) = dif_type {
        opt.dst_type = Some(dif_type);
    }
    opt.run().await?;
    // pretty_assert_eq!(
    //     &dump(&mut dif_cn).await?,
    //     databases
    //         .get(&("dif", dif_type.unwrap_or(v1_type)))
    //         .unwrap()
    // );

    for target_type in &[Flat, FlatWithHash, Normalized] {
        let trg = shorten(*target_type);
        let prefix = format!("{prefix}__to__{trg}");
        let expected_v2 = databases.get(&("v2", *target_type)).unwrap();

        info!("TEST: Applying the difference (v2-v1=diff) to v1, should get v2");
        let (tar1_mbt, mut tar1_cn) =
            new_file! {diff_apply, *target_type, METADATA_V1, TILES_V1, "{prefix}__v1"};
        apply_patch(path(&tar1_mbt), path(&dif_mbt)).await?;
        let hash_v1 = tar1_mbt.validate(Off, false).await?;
        allow_duplicates! {
            assert_display_snapshot!(hash_v1, @"5C90855D70120501451BDD08CA71341A");
        }
        let dmp = dump(&mut tar1_cn).await?;
        pretty_assert_eq!(&dmp, expected_v2);

        info!("TEST: Applying the difference (v2-v1=diff) to v2, should not modify it");
        let (tar2_mbt, mut tar2_cn) =
            new_file! {diff_apply, *target_type, METADATA_V2, TILES_V2, "{prefix}__v2"};
        apply_patch(path(&tar2_mbt), path(&dif_mbt)).await?;
        let hash_v2 = tar2_mbt.validate(Off, false).await?;
        allow_duplicates! {
            assert_display_snapshot!(hash_v2, @"5C90855D70120501451BDD08CA71341A");
        }
        let dmp = dump(&mut tar2_cn).await?;
        pretty_assert_eq!(&dmp, expected_v2);
    }

    Ok(())
}

// /// A simple tester to run specific values
// #[actix_rt::test]
// async fn test_one() {
//     let dif_type = FlatWithHash;
//     let src_type = Flat;
//     let dst_type = Some(Normalized);
//     let db = databases();
//
//     diff_apply(src_type, dif_type, dst_type, &db).await.unwrap();
//     panic!()
// }

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

#[allow(dead_code)]
async fn save_to_file(source_mbt: &Mbtiles, path: &str) -> MbtResult<()> {
    let mut opt = copier(source_mbt, &Mbtiles::new(path)?);
    opt.skip_agg_tiles_hash = true;
    opt.run().await?;
    Ok(())
}
