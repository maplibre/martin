use std::collections::HashMap;
use std::path::PathBuf;
use std::str::from_utf8;

use ctor::ctor;
use insta::{allow_duplicates, assert_display_snapshot};
use itertools::Itertools as _;
use log::info;
use martin_tile_utils::xyz_to_bbox;
use mbtiles::AggHashType::Verify;
use mbtiles::IntegrityCheckType::Off;
use mbtiles::MbtTypeCli::{Flat, FlatWithHash, Normalized};
use mbtiles::{
    apply_patch, init_mbtiles_schema, invert_y_value, CopyType, MbtResult, MbtTypeCli, Mbtiles,
    MbtilesCopier, UpdateZoomType,
};
use pretty_assertions::assert_eq as pretty_assert_eq;
use rstest::{fixture, rstest};
use serde::Serialize;
use sqlx::{query, query_as, Executor as _, Row, SqliteConnection};

const TILES_V1: &str = "
    INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES
      --(z, x, y, data) -- rules: keep if x=0, edit if x=1, remove if x=2
        (3, 6, 7, cast('root' as blob))
      , (5, 0, 0, cast('same' as blob))
      , (5, 0, 1, cast('' as blob))           -- empty tile, keep
      , (5, 1, 1, cast('edit-v1' as blob))
      , (5, 1, 2, cast('' as blob))           -- empty tile, edit
      , (5, 1, 3, cast('non-empty' as blob))  -- non empty tile to edit
      , (5, 2, 2, cast('remove' as blob))
      , (5, 2, 3, cast('' as blob))           -- empty tile, remove
      , (6, 0, 3, cast('same' as blob))
      , (6, 1, 4, cast('edit-v1' as blob))
      , (6, 0, 5, cast('1-keep-1-rm' as blob))
      , (6, 2, 6, cast('1-keep-1-rm' as blob))
      ;";

const TILES_V2: &str = "
    INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES
        (3, 6, 7, cast('root' as blob))
      , (5, 0, 0, cast('same' as blob))        -- no changes
      , (5, 0, 1, cast('' as blob))            -- no changes, empty tile
      , (5, 1, 1, cast('edit-v2' as blob))     -- edited in-place
      , (5, 1, 2, cast('not-empty' as blob))   -- edited in-place, replaced empty with non-empty
      , (5, 1, 3, cast('' as blob))            -- edited in-place, replaced non-empty with empty
   -- , (5, 2, 2, cast('remove' as blob))      -- this row is removed
   -- , (5, 2, 3, cast('' as blob))            -- empty tile, removed
      , (6, 0, 3, cast('same' as blob))        -- no changes, same content as 5/0/0
      , (6, 1, 4, cast('edit-v2a' as blob))    -- edited, used to be same as 5/1/1
      , (6, 0, 5, cast('1-keep-1-rm' as blob)) -- this row is kept (same content as next)
   -- , (6, 2, 6, cast('1-keep-1-rm' as blob)) -- this row is removed
      , (5, 3, 7, cast('new' as blob))         -- this row is added, dup value
      , (5, 3, 8, cast('new' as blob))         -- this row is added, dup value

      -- Expected delta:
      --   5/1/1 edit
      --   5/1/2 edit
      --   5/1/3 edit
      --   5/2/2 remove
      --   5/2/3 remove
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

fn shorten(v: MbtTypeCli) -> &'static str {
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
    ($function:ident, $($arg:tt)*) => {
        open!(@"", $function, $($arg)*)
    };
    (@$extra:literal, $function:tt, $($arg:tt)*) => {{
        let file = format!("file:{}_{}{}?mode=memory&cache=shared", stringify!($function), format_args!($($arg)*), $extra);
        open(&file).await.unwrap()
    }};
}

/// Create a new `SQLite` file of given type without `agg_tiles_hash` metadata value
macro_rules! new_file_no_hash {
    ($function:ident, $dst_type_cli:expr, $sql_meta:expr, $sql_data:expr, $($arg:tt)*) => {{
        new_file!(@true, $function, $dst_type_cli, $sql_meta, $sql_data, $($arg)*)
    }};
}

/// Create a new `SQLite` file of type `$dst_type_cli` with the given metadata and tiles
macro_rules! new_file {
    ($function:ident, $dst_type_cli:expr, $sql_meta:expr, $sql_data:expr, $($arg:tt)*) => {
        new_file!(@false, $function, $dst_type_cli, $sql_meta, $sql_data, $($arg)*)
    };

    (@$skip_agg:expr, $function:tt, $dst_type_cli:expr, $sql_meta:expr, $sql_data:expr, $($arg:tt)*) => {{
        let (tmp_mbt, mut cn_tmp) = open!(@"temp", $function, $($arg)*);
        init_mbtiles_schema(&mut cn_tmp, mbtiles::MbtType::Flat).await.unwrap();
        cn_tmp.execute($sql_data).await.unwrap();
        cn_tmp.execute($sql_meta).await.unwrap();

        let (dst_mbt, cn_dst) = open!($function, $($arg)*);
        let opt = MbtilesCopier {
        src_file: path(&tmp_mbt),
        dst_file: path(&dst_mbt),
        dst_type_cli: Some($dst_type_cli),
        skip_agg_tiles_hash: $skip_agg,
        ..Default::default()
    };
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

#[derive(Default)]
struct Databases(
    HashMap<(&'static str, MbtTypeCli), (Vec<SqliteEntry>, Mbtiles, SqliteConnection)>,
);

impl Databases {
    fn add(
        &mut self,
        name: &'static str,
        typ: MbtTypeCli,
        dump: Vec<SqliteEntry>,
        mbtiles: Mbtiles,
        conn: SqliteConnection,
    ) {
        self.0.insert((name, typ), (dump, mbtiles, conn));
    }
    fn dump(&self, name: &'static str, typ: MbtTypeCli) -> &Vec<SqliteEntry> {
        &self.0.get(&(name, typ)).unwrap().0
    }
    fn mbtiles(&self, name: &'static str, typ: MbtTypeCli) -> &Mbtiles {
        &self.0.get(&(name, typ)).unwrap().1
    }
}

/// Generate a set of databases for testing, and validate them against snapshot files.
/// These dbs will be used by other tests to check against in various conditions.
#[fixture]
#[once]
fn databases() -> Databases {
    futures::executor::block_on(async {
        let mut result = Databases::default();
        for &mbt_typ in &[Flat, FlatWithHash, Normalized] {
            let typ = shorten(mbt_typ);
            let (raw_mbt, mut raw_cn) = new_file_no_hash!(
                databases,
                mbt_typ,
                METADATA_V1,
                TILES_V1,
                "{typ}__v1-no-hash"
            );
            let dmp = dump(&mut raw_cn).await.unwrap();
            assert_snapshot!(&dmp, "{typ}__v1-no-hash");
            result.add("v1_no_hash", mbt_typ, dmp, raw_mbt, raw_cn);

            let (v1_mbt, mut v1_cn) = open!(databases, "{typ}__v1");
            let raw_mbt = result.mbtiles("v1_no_hash", mbt_typ);
            let opt = MbtilesCopier {
                src_file: path(raw_mbt),
                dst_file: path(&v1_mbt),
                ..Default::default()
            };
            opt.run().await.unwrap();
            let dmp = dump(&mut v1_cn).await.unwrap();
            assert_snapshot!(&dmp, "{typ}__v1");
            let hash = v1_mbt.validate(Off, Verify).await.unwrap();
            allow_duplicates! {
                assert_display_snapshot!(hash, @"9ED9178D7025276336C783C2B54D6258");
            }
            result.add("v1", mbt_typ, dmp, v1_mbt, v1_cn);

            let (v2_mbt, mut v2_cn) =
                new_file!(databases, mbt_typ, METADATA_V2, TILES_V2, "{typ}__v2");
            let dmp = dump(&mut v2_cn).await.unwrap();
            assert_snapshot!(&dmp, "{typ}__v2");
            let hash = v2_mbt.validate(Off, Verify).await.unwrap();
            allow_duplicates! {
                assert_display_snapshot!(hash, @"3BCDEE3F52407FF1315629298CB99133");
            }
            result.add("v2", mbt_typ, dmp, v2_mbt, v2_cn);

            let (dif_mbt, mut dif_cn) = open!(databases, "{typ}__dif");
            let v1_mbt = result.mbtiles("v1", mbt_typ);
            let v2_mbt = result.mbtiles("v2", mbt_typ);
            let opt = MbtilesCopier {
                src_file: path(v1_mbt),
                dst_file: path(&dif_mbt),
                diff_with_file: Some(path(v2_mbt)),
                ..Default::default()
            };
            opt.run().await.unwrap();
            let dmp = dump(&mut dif_cn).await.unwrap();
            assert_snapshot!(&dmp, "{typ}__dif");
            let hash = dif_mbt.validate(Off, Verify).await.unwrap();
            allow_duplicates! {
                assert_display_snapshot!(hash, @"B86122579EDCDD4C51F3910894FCC1A1");
            }
            result.add("dif", mbt_typ, dmp, dif_mbt, dif_cn);
        }
        result
    })
}

#[actix_rt::test]
async fn update() -> MbtResult<()> {
    let (mbt, mut cn) = new_file_no_hash!(databases, Flat, METADATA_V1, TILES_V1, "update");
    mbt.update_metadata(&mut cn, UpdateZoomType::Reset).await?;
    let dmp = dump(&mut cn).await?;
    assert_snapshot!(&dmp, "update");

    Ok(())
}

#[rstest]
#[trace]
#[actix_rt::test]
async fn convert(
    #[values(Flat, FlatWithHash, Normalized)] frm_type: MbtTypeCli,
    #[values(Flat, FlatWithHash, Normalized)] dst_type: MbtTypeCli,
    #[notrace] databases: &Databases,
) -> MbtResult<()> {
    let (frm, to) = (shorten(frm_type), shorten(dst_type));
    let mem = Mbtiles::new(":memory:")?;
    let (frm_mbt, _frm_cn) = new_file!(convert, frm_type, METADATA_V1, TILES_V1, "{frm}-{to}");

    let opt = MbtilesCopier {
        src_file: path(&frm_mbt),
        dst_file: path(&mem),
        dst_type_cli: Some(dst_type),
        ..Default::default()
    };
    let dmp = dump(&mut opt.run().await?).await?;
    pretty_assert_eq!(databases.dump("v1", dst_type), &dmp);

    let opt = MbtilesCopier {
        src_file: path(&frm_mbt),
        dst_file: path(&mem),
        copy: CopyType::Metadata,
        dst_type_cli: Some(dst_type),
        ..Default::default()
    };
    let dmp = dump(&mut opt.run().await?).await?;
    allow_duplicates! {
        assert_snapshot!(dmp, "v1__meta__{to}");
    };

    let opt = MbtilesCopier {
        src_file: path(&frm_mbt),
        dst_file: path(&mem),
        copy: CopyType::Tiles,
        dst_type_cli: Some(dst_type),
        ..Default::default()
    };
    let dmp = dump(&mut opt.run().await?).await?;
    allow_duplicates! {
        assert_snapshot!(dmp, "v1__tiles__{to}");
    }

    let opt = MbtilesCopier {
        src_file: path(&frm_mbt),
        dst_file: path(&mem),
        dst_type_cli: Some(dst_type),
        zoom_levels: vec![6],
        ..Default::default()
    };
    let z6only = dump(&mut opt.run().await?).await?;
    allow_duplicates! {
        assert_snapshot!(z6only, "v1__z6__{to}");
    }

    // Filter (0, 0, 2, 2) in mbtiles coordinates, which is (0, 2^5-1-2, 2, 2^5-1-0) = (0, 29, 2, 31) in XYZ coordinates, and slightly decrease it
    let mut bbox = xyz_to_bbox(5, 0, invert_y_value(5, 2), 2, invert_y_value(5, 0));
    let adjust = 90.0 * 0.1 / f64::from(1 << 5);
    bbox[0] += adjust;
    bbox[1] += adjust;
    bbox[2] -= adjust;
    bbox[3] -= adjust;
    let opt = MbtilesCopier {
        src_file: path(&frm_mbt),
        dst_file: path(&mem),
        dst_type_cli: Some(dst_type),
        bbox: vec![bbox.into()],
        ..Default::default()
    };
    let dmp = dump(&mut opt.run().await?).await?;
    allow_duplicates! {
        assert_snapshot!(dmp, "v1__bbox__{to}");
    }

    let opt = MbtilesCopier {
        src_file: path(&frm_mbt),
        dst_file: path(&mem),
        dst_type_cli: Some(dst_type),
        min_zoom: Some(6),
        ..Default::default()
    };
    pretty_assert_eq!(&z6only, &dump(&mut opt.run().await?).await?);

    let opt = MbtilesCopier {
        src_file: path(&frm_mbt),
        dst_file: path(&mem),
        dst_type_cli: Some(dst_type),
        min_zoom: Some(6),
        max_zoom: Some(6),
        ..Default::default()
    };
    pretty_assert_eq!(&z6only, &dump(&mut opt.run().await?).await?);

    Ok(())
}

#[rstest]
#[trace]
#[actix_rt::test]
async fn diff_and_patch(
    #[values(Flat, FlatWithHash, Normalized)] v1_type: MbtTypeCli,
    #[values(Flat, FlatWithHash, Normalized)] v2_type: MbtTypeCli,
    #[values(None, Some(Flat), Some(FlatWithHash), Some(Normalized))] dif_type: Option<MbtTypeCli>,
    #[notrace] databases: &Databases,
) -> MbtResult<()> {
    let (v1, v2) = (shorten(v1_type), shorten(v2_type));
    let dif = dif_type.map_or("dflt", shorten);
    let prefix = format!("{v2}-{v1}={dif}");

    let v1_mbt = databases.mbtiles("v1", v1_type);
    let v2_mbt = databases.mbtiles("v2", v2_type);
    let (dif_mbt, mut dif_cn) = open!(diff_and_patchdiff_and_patch, "{prefix}__dif");

    info!("TEST: Compare v1 with v2, and copy anything that's different (i.e. mathematically: v2-v1=diff)");
    let mut opt = MbtilesCopier {
        src_file: path(v1_mbt),
        dst_file: path(&dif_mbt),
        diff_with_file: Some(path(v2_mbt)),
        ..Default::default()
    };
    if let Some(dif_type) = dif_type {
        opt.dst_type_cli = Some(dif_type);
    }
    opt.run().await?;
    pretty_assert_eq!(
        &dump(&mut dif_cn).await?,
        databases.dump("dif", dif_type.unwrap_or(v1_type))
    );

    for target_type in &[Flat, FlatWithHash, Normalized] {
        let trg = shorten(*target_type);
        let prefix = format!("{prefix}__to__{trg}");
        let expected_v2 = databases.dump("v2", *target_type);

        info!("TEST: Applying the difference (v2-v1=diff) to v1, should get v2");
        let (tar1_mbt, mut tar1_cn) = new_file!(
            diff_and_patch,
            *target_type,
            METADATA_V1,
            TILES_V1,
            "{prefix}__v1"
        );
        apply_patch(path(&tar1_mbt), path(&dif_mbt)).await?;
        let hash_v1 = tar1_mbt.validate(Off, Verify).await?;
        allow_duplicates! {
            assert_display_snapshot!(hash_v1, @"3BCDEE3F52407FF1315629298CB99133");
        }
        let dmp = dump(&mut tar1_cn).await?;
        pretty_assert_eq!(&dmp, expected_v2);

        info!("TEST: Applying the difference (v2-v1=diff) to v2, should not modify it");
        let (tar2_mbt, mut tar2_cn) =
            new_file! {diff_and_patch, *target_type, METADATA_V2, TILES_V2, "{prefix}__v2"};
        apply_patch(path(&tar2_mbt), path(&dif_mbt)).await?;
        let hash_v2 = tar2_mbt.validate(Off, Verify).await?;
        allow_duplicates! {
            assert_display_snapshot!(hash_v2, @"3BCDEE3F52407FF1315629298CB99133");
        }
        let dmp = dump(&mut tar2_cn).await?;
        pretty_assert_eq!(&dmp, expected_v2);
    }

    Ok(())
}

#[rstest]
#[trace]
#[actix_rt::test]
async fn patch_on_copy(
    #[values(Flat, FlatWithHash, Normalized)] v1_type: MbtTypeCli,
    #[values(Flat, FlatWithHash, Normalized)] dif_type: MbtTypeCli,
    #[values(None, Some(Flat), Some(FlatWithHash), Some(Normalized))] v2_type: Option<MbtTypeCli>,
    #[notrace] databases: &Databases,
) -> MbtResult<()> {
    let (v1, dif) = (shorten(v1_type), shorten(dif_type));
    let v2 = v2_type.map_or("dflt", shorten);
    let prefix = format!("{v1}+{dif}={v2}");

    let v1_mbt = databases.mbtiles("v1", v1_type);
    let dif_mbt = databases.mbtiles("dif", dif_type);
    let (v2_mbt, mut v2_cn) = open!(patch_on_copy, "{prefix}__v2");

    info!("TEST: Compare v1 with v2, and copy anything that's different (i.e. mathematically: v2-v1=diff)");
    let mut opt = MbtilesCopier {
        src_file: path(v1_mbt),
        dst_file: path(&v2_mbt),
        apply_patch: Some(path(dif_mbt)),
        ..Default::default()
    };
    if let Some(v2_type) = v2_type {
        opt.dst_type_cli = Some(v2_type);
    }
    opt.run().await?;
    pretty_assert_eq!(
        &dump(&mut v2_cn).await?,
        databases.dump("v2", v2_type.unwrap_or(v1_type))
    );

    Ok(())
}

/// A simple tester to run specific values
#[actix_rt::test]
#[ignore]
async fn test_one() {
    let db = Databases::default();

    // Test convert
    convert(Flat, Flat, &db).await.unwrap();

    // Test diff patch copy
    let src_type = FlatWithHash;
    let dif_type = FlatWithHash;
    // let dst_type = Some(FlatWithHash);
    let dst_type = None;

    diff_and_patch(src_type, dif_type, dst_type, &db)
        .await
        .unwrap();
    patch_on_copy(src_type, dif_type, dst_type, &db)
        .await
        .unwrap();
    panic!("ALWAYS FAIL - this test is for debugging only, and should be disabled");
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
                let cid: u32 = row.get(0);
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
async fn save_to_file(source_mbt: &Mbtiles, path_mbt: &str) -> MbtResult<()> {
    let dst = &Mbtiles::new(path_mbt)?;
    let opt = MbtilesCopier {
        src_file: path(source_mbt),
        dst_file: path(dst),
        skip_agg_tiles_hash: true,
        ..Default::default()
    };
    opt.run().await?;
    Ok(())
}
