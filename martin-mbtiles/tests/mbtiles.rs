use std::path::PathBuf;
use std::str::from_utf8;

use ctor::ctor;
use insta::assert_toml_snapshot;
use martin_mbtiles::MbtType::{Flat, FlatWithHash, Normalized};
use martin_mbtiles::{apply_diff, create_flat_tables, MbtResult, Mbtiles, MbtilesCopier};
use serde::Serialize;
use sqlx::{query, query_as, Executor as _, Row, SqliteConnection};

const INSERT_TILES_V1: &str = "
    INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES
        (1, 0, 0, cast('same' as blob))
      , (1, 0, 1, cast('edit-v1' as blob))
      , (1, 1, 1, cast('remove' as blob))
      ;";

const INSERT_TILES_V2: &str = "
    INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES
        (1, 0, 0, cast('same' as blob))
      , (1, 0, 1, cast('edit-v2' as blob))
      , (1, 1, 0, cast('new' as blob))
      ;";

const INSERT_METADATA_V1: &str = "
    INSERT INTO metadata (name, value) VALUES
        ('md-same', 'value - same')
      , ('md-edit', 'value - v1')
      , ('md-remove', 'value - remove')
      ;";

const INSERT_METADATA_V2: &str = "
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

async fn open(file: &str) -> MbtResult<(Mbtiles, SqliteConnection)> {
    let mbtiles = Mbtiles::new(file)?;
    let conn = mbtiles.open().await?;
    Ok((mbtiles, conn))
}

macro_rules! assert_snapshot {
    ($prefix:expr, $name:expr, $actual:expr) => {
        let mut settings = insta::Settings::clone_current();
        let name = if $name != "" {
            format!("{}-{}", $prefix, $name)
        } else {
            $prefix.to_string()
        };
        settings.set_snapshot_suffix(name);
        let result = $actual;
        settings.bind(|| assert_toml_snapshot!(result));
    };
}

macro_rules! open {
    ($function:tt, $name:expr) => {{
        let func = stringify!($function);
        let file = format!("file:{func}_{}?mode=memory&cache=shared", $name);
        open(&file).await?
    }};
}

macro_rules! new_source {
    ($function:tt, $dst:expr, $sql_meta:expr, $sql_data:expr) => {{
        let func = stringify!($function);
        let name = stringify!($dst);
        let file = format!("file:{func}_{name}?mode=memory&cache=shared");
        let (dst, mut cn_dst) = open(&file).await?;
        create_flat_tables(&mut cn_dst).await?;
        cn_dst.execute($sql_data).await?;
        cn_dst.execute($sql_meta).await?;
        assert_snapshot!(name, "", dump(&mut cn_dst).await?);
        (dst, cn_dst)
    }};
}

/// Copy SQLite from $src to $dst, and add agg_tiles_hash metadata value
/// Returns the destination Mbtiles, the SQLite connection, and the dump of the SQLite
macro_rules! copy_hash {
    ($function:tt, $src:expr, $dst:tt, $dst_type:expr) => {{
        copy_to!($function, $src, $dst, $dst_type, false)
    }};
}

/// Copy SQLite from $src to $dst, no changes by default, or add agg_tiles_hash metadata if last arg is false
/// The result is dumped to a snapshot
/// Returns the destination Mbtiles, the SQLite connection, and the dump of the SQLite
macro_rules! copy_to {
    ($function:tt, $src:expr, $dst:tt, $dst_type:expr) => {{
        copy_to!($function, $src, $dst, $dst_type, true)
    }};
    ($function:tt, $src:expr, $dst:tt, $dst_type:expr, $skip_agg:expr) => {{
        let func = stringify!($function);
        let name = stringify!($dst);
        let file = format!("file:{func}--{name}?mode=memory&cache=shared");
        let (dst, cn_dst) = open(&file).await?;
        let mut opt = copier(&$src, &dst);
        opt.skip_agg_tiles_hash = $skip_agg;
        opt.dst_type = Some($dst_type);
        let dmp = dump(&mut opt.run().await?).await?;
        assert_snapshot!(name, "", &dmp);
        (dst, cn_dst, dmp)
    }};
}

#[actix_rt::test]
async fn copy_and_convert() -> MbtResult<()> {
    let mem = Mbtiles::new(":memory:")?;

    let (orig, _cn_orig) = new_source!(cp_conv, orig, INSERT_METADATA_V1, INSERT_TILES_V1);
    let (flat, _cn_flat, _) = copy_to!(cp_conv, orig, flat, Flat);
    let (hash, _cn_hash, _) = copy_to!(cp_conv, orig, hash, FlatWithHash);
    let (norm, _cn_norm, _) = copy_to!(cp_conv, orig, norm, Normalized);

    for (frm, src) in &[("flat", &flat), ("hash", &hash), ("norm", &norm)] {
        // Same content, but also will include agg_tiles_hash metadata value
        let opt = copier(src, &mem);
        assert_snapshot!(frm, "cp", dump(&mut opt.run().await?).await?);

        // Identical content to the source
        let mut opt = copier(src, &mem);
        opt.skip_agg_tiles_hash = true;
        assert_snapshot!(frm, "cp-skip-agg", dump(&mut opt.run().await?).await?);

        // Copy to a flat-with-hash schema
        let mut opt = copier(src, &mem);
        opt.dst_type = Some(FlatWithHash);
        assert_snapshot!(frm, "to-hash", dump(&mut opt.run().await?).await?);

        // Copy to a flat schema
        let mut opt = copier(src, &mem);
        opt.dst_type = Some(Flat);
        assert_snapshot!(frm, "to-flat", dump(&mut opt.run().await?).await?);

        // Copy to a normalized schema
        let mut opt = copier(src, &mem);
        opt.dst_type = Some(Normalized);
        assert_snapshot!(frm, "to-norm", dump(&mut opt.run().await?).await?);
    }

    Ok(())
}

/// Create v1 and v2 in-memory sqlite files, and copy them to flat, hash, and norm variants
/// Keep connection open to make sure they can be opened as regular files by the main code.
/// For different variants, create a delta v2-v1, and re-apply the diff to v1 to get v2a - which should be identical to v2.
#[actix_rt::test]
async fn diff_and_apply() -> MbtResult<()> {
    let (orig_v1, _cn_orig_v1) =
        new_source!(dif_aply, orig_v1, INSERT_METADATA_V1, INSERT_TILES_V1);
    let (flat_v1, _cn_flat_v1, _) = copy_hash!(dif_aply, orig_v1, flat_v1, Flat);
    let (hash_v1, _cn_hash_v1, _) = copy_hash!(dif_aply, orig_v1, hash_v1, FlatWithHash);
    let (norm_v1, _cn_norm_v1, _) = copy_hash!(dif_aply, orig_v1, norm_v1, Normalized);

    let (orig_v2, _cn_orig_v2) =
        new_source!(dif_aply, orig_v2, INSERT_METADATA_V2, INSERT_TILES_V2);
    let (flat_v2, _cn_flat_v2, dmp_flat_v2) = copy_hash!(dif_aply, orig_v2, flat_v2, Flat);
    let (hash_v2, _cn_hash_v2, dmp_hash_v2) = copy_hash!(dif_aply, orig_v2, hash_v2, FlatWithHash);
    let (norm_v2, _cn_norm_v2, dmp_norm_v2) = copy_hash!(dif_aply, orig_v2, norm_v2, Normalized);

    let types = &[
        ("flat", &flat_v1, &flat_v2, dmp_flat_v2),
        ("hash", &hash_v1, &hash_v2, dmp_hash_v2),
        ("norm", &norm_v1, &norm_v2, dmp_norm_v2),
    ];

    for (frm_type, v1, _, _) in types {
        for (to_type, _, v2, dump_v2) in types {
            let pair = format!("{frm_type}-{to_type}");
            let (dff, _cn_dff) = open!(dif_aply, format!("{pair}-dff"));
            let (v2a, mut cn_v2a) = open!(dif_aply, format!("{pair}-v2a"));

            // Diff v1 with v2, and copy to diff anything that's different (i.e. mathematically: v2-v1)
            let mut diff_with = copier(v1, &dff);
            diff_with.diff_with_file = Some(path(v2));
            assert_snapshot!("delta", pair, dump(&mut diff_with.run().await?).await?);

            // Copy v1 -> v2a, and apply dff to v2a
            copier(v1, &v2a).run().await?;
            apply_diff(path(&v2a), path(&dff)).await?;

            let dump_v2a = dump(&mut cn_v2a).await?;
            assert_snapshot!("applied", pair, &dump_v2a);

            let expected_dump = if frm_type != to_type {
                eprintln!("TODO: implement convert copying {frm_type} -> {to_type}");
                continue;
            } else if frm_type == &"norm" {
                eprintln!("FIXME: norm->norm diff is not working yet");
                continue;
            } else {
                dump_v2
            };

            pretty_assertions::assert_eq!(
                &dump_v2a,
                expected_dump,
                "v2a should be identical to v2 (type {frm_type} -> {to_type})"
            );
        }
    }

    Ok(())
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
