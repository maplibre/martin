use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::str::from_utf8;
use std::sync::Mutex;

use ctor::ctor;
use insta::{allow_duplicates, assert_snapshot};
use itertools::Itertools as _;
use martin_tile_utils::xyz_to_bbox;
use mbtiles::AggHashType::Verify;
use mbtiles::IntegrityCheckType::Off;
use mbtiles::MbtTypeCli::{Flat, FlatWithHash, Normalized};
use mbtiles::PatchTypeCli::{BinDiffGz, BinDiffRaw};
use mbtiles::{
    CopyType, MbtResult, MbtTypeCli, Mbtiles, MbtilesCopier, PatchTypeCli, UpdateZoomType,
    apply_patch, init_mbtiles_schema, invert_y_value,
};
use pretty_assertions::assert_eq as pretty_assert_eq;
use rstest::{fixture, rstest};
use serde::Serialize;
use sqlx::{Executor as _, Row, SqliteConnection, query, query_as};
use tokio::runtime::Handle;

const GZIP_TILES: &str = "UPDATE tiles SET tile_data = gzip(tile_data);";

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

/// Open an `MBTiles` file, returning both the `MBTiles` and the `SqliteConnection`.
async fn open(file: &str) -> MbtResult<(Mbtiles, SqliteConnection)> {
    let mbtiles = Mbtiles::new(file)?;
    let conn = mbtiles.open().await?;
    Ok((mbtiles, conn))
}

/// Run [`MbtilesCopier`], the first two params are source and destination [`Mbtiles`] refs, the rest are optional (key => val)* params.
macro_rules! copy {
    ($src_path:expr, $dst_path:expr $( , $key:tt => $val:expr )* $(,)?) => {
        MbtilesCopier {
            src_file: $src_path,
            dst_file: $dst_path
            $(, $key : $val)*,
            ..Default::default()
        }.run().await.unwrap()
    };
}

/// Same as the copy! macro, but with the result dumped.
macro_rules! copy_dump {
    ($src_path:expr, $dst_path:expr $( , $key:tt => $val:expr )* $(,)?) => {{
        dump(&mut copy!($src_path, $dst_path $(, $key => $val)*)).await.unwrap()
    }};
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
        new_file!(@true, $function, $dst_type_cli, $sql_meta, $sql_data, "", $($arg)*)
    }};
}

/// Create a new `SQLite` file of type `$dst_type_cli` with the given metadata and tiles
macro_rules! new_file {
    ($function:ident, $dst_type_cli:expr, $sql_meta:expr, $sql_data:expr, $($arg:tt)*) => {
        new_file!(@false, $function, $dst_type_cli, $sql_meta, $sql_data, "", $($arg)*)
    };

    (+ $sql:expr, $function:ident, $dst_type_cli:expr, $sql_meta:expr, $sql_data:expr, $($arg:tt)*) => {
        new_file!(@false, $function, $dst_type_cli, $sql_meta, $sql_data, $sql, $($arg)*)
    };

    (@ $skip_agg:expr, $function:tt, $dst_type_cli:expr, $sql_meta:expr, $sql_data:expr, $sql:expr, $($arg:tt)*) => {{
        let (tmp_mbt, mut cn_tmp) = open!(@"temp", $function, $($arg)*);
        init_mbtiles_schema(&mut cn_tmp, mbtiles::MbtType::Flat).await.unwrap();
        cn_tmp.execute($sql_data).await.unwrap();
        cn_tmp.execute($sql_meta).await.unwrap();
        if $sql != "" {
            cn_tmp.execute($sql).await.unwrap();
        }

        let (dst_mbt, cn_dst) = open!($function, $($arg)*);
        copy! {
            path(&tmp_mbt),
            path(&dst_mbt),
            dst_type_cli => Some($dst_type_cli),
            skip_agg_tiles_hash => $skip_agg,
        };

        (dst_mbt, cn_dst)
    }};
}

macro_rules! assert_dump {
    ($actual_value:expr, $($arg:tt)*) => {{
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_suffix(format!($($arg)*));
        let actual_value = &$actual_value;
        settings.bind(||
            allow_duplicates! {
                insta::assert_toml_snapshot!(actual_value)
            }
        );
    }};
}

#[derive(Default)]
#[allow(clippy::type_complexity)]
struct Databases(
    HashMap<
        (&'static str, MbtTypeCli),
        (
            Vec<SqliteEntry>,
            Mbtiles,
            Option<String>,
            Mutex<SqliteConnection>,
        ),
    >,
);

impl Databases {
    fn add(
        &mut self,
        name: &'static str,
        typ: MbtTypeCli,
        dump: Vec<SqliteEntry>,
        mbtiles: Mbtiles,
        hash: Option<String>,
        conn: SqliteConnection,
    ) {
        self.0
            .insert((name, typ), (dump, mbtiles, hash, Mutex::new(conn)));
    }
    fn dump(&self, name: &'static str, typ: MbtTypeCli) -> &Vec<SqliteEntry> {
        &self.0.get(&(name, typ)).unwrap().0
    }
    fn mbtiles(&self, name: &'static str, typ: MbtTypeCli) -> &Mbtiles {
        &self.0.get(&(name, typ)).unwrap().1
    }
    fn path(&self, name: &'static str, typ: MbtTypeCli) -> PathBuf {
        path(self.mbtiles(name, typ))
    }
    fn hash(&self, name: &'static str, typ: MbtTypeCli) -> &str {
        self.0.get(&(name, typ)).unwrap().2.as_deref().unwrap()
    }
}

/// Generate a set of databases for testing, and validate them against snapshot files.
/// These dbs will be used by other tests to check against in various conditions.
#[fixture]
#[once]
#[allow(clippy::too_many_lines)]
fn databases() -> Databases {
    tokio::task::block_in_place(|| {
        Handle::current().block_on(async {
            let mut result = Databases::default();
            for &mbt_typ in &[Flat, FlatWithHash, Normalized] {
                let typ = shorten(mbt_typ);

                // ----------------- empty_no_hash -----------------
                let (raw_empty_mbt, mut raw_empty_cn) =
                    new_file_no_hash!(databases, mbt_typ, "", "", "{typ}__empty-no-hash");
                let dmp = dump(&mut raw_empty_cn).await.unwrap();
                assert_dump!(&dmp, "{typ}__empty-no-hash");
                result.add(
                    "empty_no_hash",
                    mbt_typ,
                    dmp,
                    raw_empty_mbt,
                    None,
                    raw_empty_cn,
                );

                // ----------------- empty -----------------
                let (empty_mbt, mut empty_cn) = open!(databases, "{typ}__empty");
                copy!(result.path("empty_no_hash", mbt_typ), path(&empty_mbt));
                let dmp = dump(&mut empty_cn).await.unwrap();
                assert_dump!(&dmp, "{typ}__empty");
                let hash = empty_mbt.open_and_validate(Off, Verify).await.unwrap();
                allow_duplicates! {
                    assert_snapshot!(hash, @"D41D8CD98F00B204E9800998ECF8427E");
                }
                result.add("empty", mbt_typ, dmp, empty_mbt, Some(hash), empty_cn);

                // ----------------- v1_no_hash -----------------
                let (raw_mbt, mut raw_cn) = new_file_no_hash!(
                    databases,
                    mbt_typ,
                    METADATA_V1,
                    TILES_V1,
                    "{typ}__v1-no-hash"
                );
                let dmp = dump(&mut raw_cn).await.unwrap();
                assert_dump!(&dmp, "{typ}__v1-no-hash");
                result.add("v1_no_hash", mbt_typ, dmp, raw_mbt, None, raw_cn);

                // ----------------- v1 -----------------
                let (v1_mbt, mut v1_cn) = open!(databases, "{typ}__v1");
                copy!(result.path("v1_no_hash", mbt_typ), path(&v1_mbt));
                let dmp = dump(&mut v1_cn).await.unwrap();
                assert_dump!(&dmp, "{typ}__v1");
                let hash = v1_mbt.open_and_validate(Off, Verify).await.unwrap();
                allow_duplicates! {
                    assert_snapshot!(hash, @"9ED9178D7025276336C783C2B54D6258");
                }
                result.add("v1", mbt_typ, dmp, v1_mbt, Some(hash), v1_cn);

                // ----------------- v2 -----------------
                let (v2_mbt, mut v2_cn) =
                    new_file!(databases, mbt_typ, METADATA_V2, TILES_V2, "{typ}__v2");
                let dmp = dump(&mut v2_cn).await.unwrap();
                assert_dump!(&dmp, "{typ}__v2");
                let hash = v2_mbt.open_and_validate(Off, Verify).await.unwrap();
                allow_duplicates! {
                    assert_snapshot!(hash, @"3BCDEE3F52407FF1315629298CB99133");
                }
                result.add("v2", mbt_typ, dmp, v2_mbt, Some(hash), v2_cn);

                // ----------------- dif (v1 -> v2) -----------------
                let (dif_mbt, mut dif_cn) = open!(databases, "{typ}__dif");
                copy! {
                    result.path("v1", mbt_typ),
                    path(&dif_mbt),
                    diff_with_file => Some((result.path("v2", mbt_typ), None)),
                };
                let dmp = dump(&mut dif_cn).await.unwrap();
                assert_dump!(&dmp, "{typ}__dif");
                let hash = dif_mbt.open_and_validate(Off, Verify).await.unwrap();
                allow_duplicates! {
                    assert_snapshot!(hash, @"B86122579EDCDD4C51F3910894FCC1A1");
                }
                result.add("dif", mbt_typ, dmp, dif_mbt, Some(hash), dif_cn);

                // ----------------- v1z -----------------
                let (v1z_mbt, mut v1z_cn) =
                    new_file!(+GZIP_TILES, databases, mbt_typ, METADATA_V1, TILES_V1, "{typ}__v1z");
                let dmp = dump(&mut v1z_cn).await.unwrap();
                assert_dump!(&dmp, "{typ}__v1z");
                let hash = v1z_mbt.open_and_validate(Off, Verify).await.unwrap();
                allow_duplicates! {
                    assert_snapshot!(hash, @"C0CA886B149CE416242AB2AFE8E641AD");
                }
                result.add("v1z", mbt_typ, dmp, v1z_mbt, Some(hash), v1z_cn);

                // ----------------- v2z -----------------
                let (v2z_mbt, mut v2z_cn) =
                    new_file!(+GZIP_TILES, databases, mbt_typ, METADATA_V2, TILES_V2, "{typ}__v2z");
                let dmp = dump(&mut v2z_cn).await.unwrap();
                assert_dump!(&dmp, "{typ}__v2z");
                let hash = v2z_mbt.open_and_validate(Off, Verify).await.unwrap();
                allow_duplicates! {
                    assert_snapshot!(hash, @"A18D0C39730FB52E5A547F096F5C60E8");
                }
                result.add("v2z", mbt_typ, dmp, v2z_mbt, Some(hash), v2z_cn);

                // ----------------- bin-diff (v1 -> v2) -----------------
                if mbt_typ == Flat || mbt_typ == FlatWithHash {
                    for (a, b, patch_type, pt) in [
                        ("v1", "v2", BinDiffRaw, "bdr"),
                        ("v1z", "v2z", BinDiffGz, "bdz"),
                    ] {
                        let (bd_mbt, mut bd_cn) = open!(databases, "{typ}__{pt}");
                        copy! {
                            result.path(a, mbt_typ),
                            path(&bd_mbt),
                            diff_with_file => Some((result.path(b, mbt_typ), patch_type.into())),
                        };
                        let dmp = dump(&mut bd_cn).await.unwrap();
                        assert_dump!(&dmp, "{typ}__{pt}");
                        let hash = bd_mbt.open_and_validate(Off, Verify).await.unwrap();
                        match patch_type {
                            PatchTypeCli::Whole => {
                                unreachable!()
                            }
                            BinDiffGz => {
                                allow_duplicates!(
                                    assert_snapshot!(hash, @"9AFEC3326B465CB939664C47A572D4C6")
                                );
                            }
                            BinDiffRaw => {
                                allow_duplicates!(
                                    assert_snapshot!(hash, @"585A88FEEC740448FF1EB4F96088FFE3")
                                );
                            }
                        }
                        result.add(pt, mbt_typ, dmp, bd_mbt, Some(hash), bd_cn);
                    }
                }

                // ----------------- v1_clone -----------------
                let (v1_clone_mbt, v1_clone_cn) = open!(databases, "{typ}__v1-clone");
                let dmp = copy_dump!(result.path("v1", mbt_typ), path(&v1_clone_mbt));
                let hash = v1_clone_mbt.open_and_validate(Off, Verify).await.unwrap();
                allow_duplicates! {
                    assert_snapshot!(hash, @"9ED9178D7025276336C783C2B54D6258");
                }
                result.add(
                    "v1_clone",
                    mbt_typ,
                    dmp,
                    v1_clone_mbt,
                    Some(hash),
                    v1_clone_cn,
                );

                // ----------------- dif_empty (v1 -> v1_clone) -----------------
                let (dif_empty_mbt, mut dif_empty_cn) = open!(databases, "{typ}__dif_empty");
                copy! {
                    result.path("v1", mbt_typ),
                    path(&dif_empty_mbt),
                    diff_with_file => Some((result.path("v1_clone", mbt_typ), None)),
                };
                let dmp = dump(&mut dif_empty_cn).await.unwrap();
                assert_dump!(&dmp, "{typ}__dif_empty");
                let hash = dif_empty_mbt.open_and_validate(Off, Verify).await.unwrap();
                allow_duplicates! {
                    assert_snapshot!(hash, @"D41D8CD98F00B204E9800998ECF8427E");
                }
                result.add(
                    "dif_empty",
                    mbt_typ,
                    dmp,
                    dif_empty_mbt,
                    Some(hash),
                    dif_empty_cn,
                );
            }
            result
        })
    })
}

#[tokio::test]
async fn update() -> MbtResult<()> {
    let (mbt, mut cn) = new_file_no_hash!(databases, Flat, METADATA_V1, TILES_V1, "update");
    mbt.update_metadata(&mut cn, UpdateZoomType::Reset).await?;
    let dmp = dump(&mut cn).await?;
    assert_dump!(&dmp, "update");

    Ok(())
}

#[rstest]
#[trace]
#[tokio::test(flavor = "multi_thread")]
async fn convert(
    #[values(Flat, FlatWithHash, Normalized)] frm_type: MbtTypeCli,
    #[values(Flat, FlatWithHash, Normalized)] dst_type: MbtTypeCli,
    #[notrace] databases: &Databases,
) -> MbtResult<()> {
    let (frm, to) = (shorten(frm_type), shorten(dst_type));
    let mem = Mbtiles::new(":memory:")?;
    let (frm_mbt, _frm_cn) = new_file!(convert, frm_type, METADATA_V1, TILES_V1, "{frm}-{to}");

    pretty_assert_eq!(
        databases.dump("v1", dst_type),
        &copy_dump! {
            path(&frm_mbt),
            path(&mem),
            dst_type_cli => Some(dst_type),
        }
    );

    let dmp = copy_dump! {
        path(&frm_mbt),
        path(&mem),
        copy => CopyType::Metadata,
        dst_type_cli => Some(dst_type),
    };
    assert_dump!(dmp, "v1__meta__{to}");

    let dmp = copy_dump! {
        path(&frm_mbt),
        path(&mem),
        copy => CopyType::Tiles,
        dst_type_cli => Some(dst_type),
    };
    assert_dump!(dmp, "v1__tiles__{to}");

    let z6only = copy_dump! {
        path(&frm_mbt),
        path(&mem),
        dst_type_cli => Some(dst_type),
        zoom_levels => vec![6],
    };
    assert_dump!(z6only, "v1__z6__{to}");

    // Filter (0, 0, 2, 2) in mbtiles coordinates, which is (0, 2^5-1-2, 2, 2^5-1-0) = (0, 29, 2, 31) in XYZ coordinates, and slightly decrease it
    let mut bbox = xyz_to_bbox(5, 0, invert_y_value(5, 2), 2, invert_y_value(5, 0));
    let adjust = 90.0 * 0.1 / f64::from(1 << 5);
    bbox[0] += adjust;
    bbox[1] += adjust;
    bbox[2] -= adjust;
    bbox[3] -= adjust;
    let dmp = copy_dump! {
        path(&frm_mbt),
        path(&mem),
        dst_type_cli => Some(dst_type),
        bbox => vec![bbox.into()],
    };
    assert_dump!(dmp, "v1__bbox__{to}");

    pretty_assert_eq!(
        &z6only,
        &copy_dump! {
            path(&frm_mbt),
            path(&mem),
            dst_type_cli => Some(dst_type),
            min_zoom => Some(6),
        }
    );

    pretty_assert_eq!(
        &z6only,
        &copy_dump! {
            path(&frm_mbt),
            path(&mem),
            dst_type_cli => Some(dst_type),
            min_zoom => Some(6),
            max_zoom => Some(6),
        }
    );

    Ok(())
}

#[rstest]
#[trace]
#[tokio::test(flavor = "multi_thread")]
async fn diff_and_patch(
    #[values(Flat, FlatWithHash, Normalized)] a_type: MbtTypeCli,
    #[values(Flat, FlatWithHash, Normalized)] b_type: MbtTypeCli,
    #[values(None, Some(Flat), Some(FlatWithHash), Some(Normalized))] dif_type: Option<MbtTypeCli>,
    #[values(&[Flat, FlatWithHash, Normalized])] destination_types: &[MbtTypeCli],
    #[values(
        ("v1", "v2", "dif"),
        ("v1", "v1_clone", "dif_empty"))]
    tilesets: (&'static str, &'static str, &'static str),
    #[notrace] databases: &Databases,
) -> MbtResult<()> {
    let (a_db, b_db, dif_db) = tilesets;
    let dif = dif_type.map_or("dflt", shorten);
    let prefix = format!(
        "{a_db}_{}--{b_db}_{}={dif}",
        shorten(b_type),
        shorten(a_type),
    );

    eprintln!(
        "TEST: Compare {a_db} with {b_db}, and copy anything that's different (i.e. mathematically: {b_db} - {a_db} = {dif_db})"
    );
    let (dif_mbt, mut dif_cn) = open!(diff_and_patch, "{prefix}__{dif_db}");
    copy! {
        databases.path(a_db, a_type),
        path(&dif_mbt),
        diff_with_file => Some((databases.path(b_db, b_type), None)),
        dst_type_cli => dif_type,
    };
    pretty_assert_eq!(
        &dump(&mut dif_cn).await?,
        databases.dump(dif_db, dif_type.unwrap_or(a_type))
    );

    for dst_type in destination_types {
        let prefix = format!("{prefix}__to__{}", shorten(*dst_type));
        let expected_b = databases.dump(b_db, *dst_type);

        eprintln!(
            "TEST: Applying the difference ({b_db} - {a_db} = {dif_db}) to {a_db}, should get {b_db}"
        );
        let (clone_mbt, mut clone_cn) = open!(diff_and_patch, "{prefix}__1");
        copy!(databases.path(a_db, *dst_type), path(&clone_mbt));
        apply_patch(path(&clone_mbt), path(&dif_mbt), false).await?;
        let hash = clone_mbt.open_and_validate(Off, Verify).await?;
        assert_eq!(hash, databases.hash(b_db, *dst_type));
        let dmp = dump(&mut clone_cn).await?;
        pretty_assert_eq!(&dmp, expected_b);

        eprintln!(
            "TEST: Applying the difference ({b_db} - {a_db} = {dif_db}) to {b_db}, should not modify it"
        );
        let (clone_mbt, mut clone_cn) = open!(diff_and_patch, "{prefix}__2");
        copy!(databases.path(b_db, *dst_type), path(&clone_mbt));
        apply_patch(path(&clone_mbt), path(&dif_mbt), true).await?;
        let hash = clone_mbt.open_and_validate(Off, Verify).await?;
        assert_eq!(hash, databases.hash(b_db, *dst_type));
        let dmp = dump(&mut clone_cn).await?;
        pretty_assert_eq!(&dmp, expected_b);
    }

    Ok(())
}

#[rstest]
#[trace]
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn diff_and_patch_bsdiff(
    #[values(Flat, FlatWithHash)] a_type: MbtTypeCli,
    #[values(Flat, FlatWithHash)] b_type: MbtTypeCli,
    #[values(Flat, FlatWithHash)] dif_type: MbtTypeCli,
    #[values(Flat, FlatWithHash)] dst_type: MbtTypeCli,
    #[values(
        ("v1", "v2", "bdr", BinDiffRaw),
        ("v1z", "v2z", "bdz", BinDiffGz),
    )]
    tilesets: (&'static str, &'static str, &'static str, PatchTypeCli),
    #[notrace] databases: &Databases,
) -> MbtResult<()> {
    let (a_db, b_db, dif_db, patch_type) = tilesets;
    let dif = shorten(dif_type);
    let prefix = format!(
        "{a_db}_{}--{b_db}_{}={dif}_{patch_type}",
        shorten(b_type),
        shorten(a_type),
    );

    eprintln!(
        "TEST: Compare {a_db} with {b_db}, and copy anything that's different (i.e. mathematically: {b_db} - {a_db} = {dif_db})"
    );
    let (dif_mbt, mut dif_cn) = open!(diff_and_patch_bsdiff, "{prefix}__{dif_db}");
    copy! {
        databases.path(a_db, a_type),
        path(&dif_mbt),
        diff_with_file => Some((databases.path(b_db, b_type), patch_type.into())),
        dst_type_cli => Some(dif_type),
    };
    pretty_assert_eq!(&dump(&mut dif_cn).await?, databases.dump(dif_db, dif_type));

    let prefix = format!("{prefix}__to__{}", shorten(dst_type));
    let (b_mbt, mut b_cn) = open!(diff_and_patch_bsdiff, "{prefix}__{b_db}");
    copy! {
        databases.path(a_db, a_type),
        path(&b_mbt),
        apply_patch => Some(databases.path(dif_db, dif_type)),
        dst_type_cli => Some(dst_type),
    };
    let actual = dump(&mut b_cn).await?;
    let expected = databases.dump(b_db, dst_type);
    pretty_assert_eq!(&actual, expected);

    Ok(())
}

#[rstest]
#[trace]
#[tokio::test(flavor = "multi_thread")]
async fn patch_on_copy(
    #[values(Flat, FlatWithHash, Normalized)] v1_type: MbtTypeCli,
    #[values(Flat, FlatWithHash, Normalized)] dif_type: MbtTypeCli,
    #[values(None, Some(Flat), Some(FlatWithHash), Some(Normalized))] v2_type: Option<MbtTypeCli>,
    #[notrace] databases: &Databases,
) -> MbtResult<()> {
    let (v1, dif) = (shorten(v1_type), shorten(dif_type));
    let v2 = v2_type.map_or("dflt", shorten);
    let prefix = format!("{v1}+{dif}={v2}");

    let (v2_mbt, mut v2_cn) = open!(patch_on_copy, "{prefix}__v2");
    copy! {
        databases.path("v1", v1_type),
        path(&v2_mbt),
        apply_patch => Some(databases.path("dif", dif_type)),
        dst_type_cli => v2_type,
    };
    let actual = dump(&mut v2_cn).await?;
    let expected = databases.dump("v2", v2_type.unwrap_or(v1_type));
    pretty_assert_eq!(&actual, expected);

    Ok(())
}

/// A simple tester to run specific values
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_one() {
    // This will cause an error if ran together with other tests
    let db = databases();
    // let db = Databases::default();

    // Test convert
    // convert(Flat, Flat, &db).await.unwrap();

    // diff_and_patch(
    //     FlatWithHash,
    //     FlatWithHash,
    //     None,
    //     &[Flat],
    //     ("v1", "v2", "dif"),
    //     &db,
    // )
    // .await
    // .unwrap();

    diff_and_patch_bsdiff(
        FlatWithHash,
        FlatWithHash,
        FlatWithHash,
        FlatWithHash,
        ("v1", "v2", "bdr", BinDiffRaw),
        &db,
    )
    .await
    .unwrap();

    // patch_on_copy(FlatWithHash, FlatWithHash, None, &db)
    //     .await
    //     .unwrap();

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
                        // if let Ok(v) = row.try_get::<i64, _>(idx) {
                        //     return v.to_string();
                        // }
                        // if let Ok(v) = row.try_get::<f64, _>(idx) {
                        //     return v.to_string();
                        // }
                        // panic!("Unknown column type: {typ}");
                        (match typ.as_str() {
                            "INTEGER" => row.get::<Option<i64>, _>(idx).map(|v| v.to_string()),
                            "REAL" => row.get::<Option<f64>, _>(idx).map(|v| v.to_string()),
                            "TEXT" => row
                                .get::<Option<String>, _>(idx)
                                .map(|v| format!(r#""{v}""#)),
                            "BLOB" => row.get::<Option<Vec<u8>>, _>(idx).map(|v| {
                                format!(
                                    "blob({})",
                                    from_utf8(&v).map_or_else(
                                        |_| {
                                            v.iter().fold(String::new(), |mut out, b| {
                                                let _ = write!(out, "{b:02X}");
                                                out
                                            })
                                        },
                                        ToString::to_string,
                                    )
                                )
                            }),
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
async fn save_to_file(source_mbt: &Mbtiles, path_mbt: &str) {
    copy!(
        path(source_mbt),
        path(&Mbtiles::new(path_mbt).unwrap()),
        skip_agg_tiles_hash => true,
    );
}
