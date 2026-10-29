//! A tile index: a `GeoPackage` with one footprint and location per file, as written by `gdaltindex`.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::TryStreamExt as _;
use geo_index::rtree::sort::HilbertSort;
use geo_index::rtree::{RTree, RTreeBuilder, RTreeIndex as _};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions as _, Row as _, SqliteConnection};

use crate::{
    AsyncIndexedMosaic, CogError, CogResult, HrefResolver, IndexedMosaicOptions, MosaicFile, MosaicIndex, TileCoord,
    mercator_to_lnglat,
};

/// The `location` column `gdaltindex` writes when `-tileindex` is not given.
pub const DEFAULT_LOCATION_FIELD: &str = "location";

/// Finds the files of a tile by their footprints, read once from a tile index into memory.
pub struct TileIndex {
    /// The index file, as shown in errors.
    path: PathBuf,
    /// The bounding boxes of the files' footprints, by index into `files`.
    tree: RTree<f64>,
    /// The files, in the index's row order, which is the order they are painted in.
    files: Vec<MosaicFile>,
}

impl fmt::Debug for TileIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TileIndex")
            .field("path", &self.path)
            .field("files", &self.files.len())
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl MosaicIndex for TileIndex {
    async fn files_for(&self, coord: TileCoord) -> CogResult<Arc<[MosaicFile]>> {
        let [min_x, min_y, max_x, max_y] = coord.mercator_bounds();
        let [west, south] = mercator_to_lnglat(min_x, min_y);
        let [east, north] = mercator_to_lnglat(max_x, max_y);
        let mut hits = self.tree.search(west, south, east, north);
        hits.sort_unstable();
        Ok(hits.into_iter().map(|hit| self.files[hit as usize].clone()).collect())
    }

    fn bounds(&self) -> Option<[f64; 4]> {
        Some(self.files.iter().map(|file| file.bounds).fold(
            [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY],
            |[w, s, e, n], [fw, fs, fe, fn_]| [w.min(fw), s.min(fs), e.max(fe), n.max(fn_)],
        ))
    }
}

impl TileIndex {
    /// Reads the footprints and locations of `layer` (or the only feature table) of the `GeoPackage` at
    /// `path`, resolving each location with `resolve`.
    ///
    /// Locations may be URLs, GDAL `/vsis3/`, `/vsigs/`, `/vsiaz/` or `/vsicurl/` paths, or local paths,
    /// which are relative to the index file's directory unless absolute.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read, has no such layer, has footprints that are not in
    /// `EPSG:4326` or lack an envelope, has a location that cannot be resolved, or has no files.
    pub async fn open(
        path: &Path,
        layer: Option<&str>,
        location_field: &str,
        resolve: &HrefResolver,
    ) -> CogResult<Self> {
        let read_error = |error| CogError::TileIndexRead {
            path: path.to_path_buf(),
            error,
        };
        let mut conn = SqliteConnectOptions::new()
            .filename(path)
            .read_only(true)
            .connect()
            .await
            .map_err(read_error)?;
        let layer = match layer {
            Some(layer) => layer.to_owned(),
            None => only_feature_table(&mut conn, path).await?,
        };
        let geometry_column = geometry_column(&mut conn, path, &layer).await?;

        // Identifiers are quoted, so names from the config cannot change the statement.
        let sql = format!(
            "SELECT {}, {} FROM {} ORDER BY rowid",
            quoted(location_field),
            quoted(&geometry_column),
            quoted(&layer),
        );
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let mut files = Vec::new();
        let mut rows = sqlx::query(sqlx::AssertSqlSafe(sql)).fetch(&mut conn);
        while let Some(row) = rows.try_next().await.map_err(read_error)? {
            let location: String = row.try_get(0).map_err(read_error)?;
            let footprint: Vec<u8> = row.try_get(1).map_err(read_error)?;
            let url = location_url(&location, base).ok_or_else(|| CogError::TileIndexLocation {
                path: path.to_path_buf(),
                location: location.clone(),
                error: "it is neither a URL, a supported GDAL virtual path, nor a local path".into(),
            })?;
            let shown = shown_location(&location, &url);
            let bounds = envelope(&footprint).ok_or_else(|| CogError::TileIndexFootprint {
                path: path.to_path_buf(),
                location: shown.clone(),
            })?;
            let (store, store_path) = resolve(&url).map_err(|error| CogError::TileIndexLocation {
                path: path.to_path_buf(),
                location: shown.clone(),
                error,
            })?;
            files.push(MosaicFile {
                cache_key: url.into(),
                location: shown.into(),
                store,
                path: store_path,
                bounds,
            });
        }
        drop(rows);

        if files.is_empty() {
            return Err(CogError::TileIndexEmpty {
                path: path.to_path_buf(),
                layer,
            });
        }
        let count = u32::try_from(files.len()).map_err(|_too_many| CogError::TileIndexTooLarge {
            path: path.to_path_buf(),
            count: files.len(),
        })?;
        let mut builder = RTreeBuilder::<f64>::new(count);
        for &MosaicFile {
            bounds: [west, south, east, north],
            ..
        } in &files
        {
            builder.add(west, south, east, north);
        }
        Ok(Self {
            path: path.to_path_buf(),
            tree: builder.finish::<HilbertSort>(),
            files,
        })
    }
}

impl AsyncIndexedMosaic {
    /// Opens a mosaic of the files `index` lists, checking now that the first can be served.
    ///
    /// # Errors
    /// See [`open`](Self::open).
    pub async fn open_tile_index(index: TileIndex, options: IndexedMosaicOptions) -> CogResult<Self> {
        let probe = index.files[0].clone();
        Self::open(Arc::new(index), &probe, options).await
    }
}

/// The name of the only feature table in the `GeoPackage`.
async fn only_feature_table(conn: &mut SqliteConnection, path: &Path) -> CogResult<String> {
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT table_name FROM gpkg_contents WHERE data_type = 'features' ORDER BY table_name")
            .fetch_all(&mut *conn)
            .await
            .map_err(|error| CogError::TileIndexRead {
                path: path.to_path_buf(),
                error,
            })?;
    match <[String; 1]>::try_from(tables) {
        Ok([table]) => Ok(table),
        Err(tables) => Err(CogError::TileIndexLayer {
            path: path.to_path_buf(),
            layers: tables.join(", "),
        }),
    }
}

/// The geometry column of `layer`, which must be in `EPSG:4326`.
async fn geometry_column(conn: &mut SqliteConnection, path: &Path, layer: &str) -> CogResult<String> {
    let row: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT g.column_name, s.organization, s.organization_coordsys_id
         FROM gpkg_geometry_columns g JOIN gpkg_spatial_ref_sys s ON s.srs_id = g.srs_id
         WHERE g.table_name = ?",
    )
    .bind(layer)
    .fetch_optional(&mut *conn)
    .await
    .map_err(|error| CogError::TileIndexRead {
        path: path.to_path_buf(),
        error,
    })?;
    let Some((column, organization, code)) = row else {
        return Err(CogError::TileIndexNoLayer {
            path: path.to_path_buf(),
            layer: layer.to_owned(),
        });
    };
    if !organization.eq_ignore_ascii_case("EPSG") || code != 4326 {
        return Err(CogError::TileIndexSrs {
            path: path.to_path_buf(),
            layer: layer.to_owned(),
            srs: format!("{organization}:{code}"),
        });
    }
    Ok(column)
}

/// The `[west, south, east, north]` envelope stored in a `GeoPackage` geometry header, if any.
fn envelope(blob: &[u8]) -> Option<[f64; 4]> {
    let [b'G', b'P', _version, flags, ..] = *blob else {
        return None;
    };
    let empty = flags & 0b1_0000 != 0;
    let has_envelope = (flags >> 1) & 0b111 != 0;
    if empty || !has_envelope {
        return None;
    }
    let little_endian = flags & 1 == 1;
    let value = |i: usize| {
        let bytes: [u8; 8] = blob.get(8 + i * 8..16 + i * 8)?.try_into().ok()?;
        Some(if little_endian {
            f64::from_le_bytes(bytes)
        } else {
            f64::from_be_bytes(bytes)
        })
    };
    let [min_x, max_x, min_y, max_y] = [value(0)?, value(1)?, value(2)?, value(3)?];
    Some([min_x, min_y, max_x, max_y])
}

/// The URL of a tile index `location`, resolving local paths against `base`.
fn location_url(location: &str, base: &Path) -> Option<String> {
    for (prefix, scheme) in [("/vsis3/", "s3://"), ("/vsigs/", "gs://"), ("/vsiaz/", "az://")] {
        if let Some(rest) = location.strip_prefix(prefix) {
            return Some(format!("{scheme}{rest}"));
        }
    }
    if let Some(rest) = location.strip_prefix("/vsicurl/") {
        return Some(rest.to_owned());
    }
    if location.starts_with("/vsi") {
        return None;
    }
    if url::Url::parse(location).is_ok_and(|url| url.scheme().len() > 1) {
        return Some(location.to_owned());
    }
    let path = base.join(location);
    let path = std::path::absolute(&path).ok()?;
    url::Url::from_file_path(path).ok().map(String::from)
}

/// How a file `location` resolved to `url` is shown in errors: a remote URL without credentials,
/// query or fragment (e.g. a signature), or a local path as the index writes it.
fn shown_location(location: &str, url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut url) if url.scheme() != "file" => {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            url.to_string()
        }
        _ => location.to_owned(),
    }
}

/// `name` as an SQL identifier.
fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::{MosaicIndex as _, TileCoord};
    use object_store::ObjectStore;
    use object_store::memory::InMemory;
    use object_store::path::Path;
    use sqlx::{ConnectOptions as _, sqlite::SqliteConnectOptions};

    use crate::{HrefResolver, TileIndex};

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/cog/tile_index/naip.gpkg")
    }

    #[tokio::test]
    async fn signed_urls_are_shown_without_credentials_query_or_fragment() {
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("signed.gpkg");
        std::fs::copy(fixture(), &index).unwrap();
        let mut conn = SqliteConnectOptions::new().filename(&index).connect().await.unwrap();
        let triggers: Vec<String> =
            sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'local'")
                .fetch_all(&mut conn)
                .await
                .unwrap();
        for trigger in triggers {
            sqlx::query(sqlx::AssertSqlSafe(format!("DROP TRIGGER \"{trigger}\"")))
                .execute(&mut conn)
                .await
                .unwrap();
        }
        sqlx::query("UPDATE local SET location = 'https://user:secret@example.com/naip/' || fid || '.tif?X-Amz-Signature=abc#part'")
            .execute(&mut conn)
            .await
            .unwrap();
        drop(conn);
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let resolve: HrefResolver = Arc::new(move |href: &str| Ok((Arc::clone(&store), Path::from(href))));

        let index = TileIndex::open(&index, Some("local"), "location", &resolve)
            .await
            .unwrap();
        let files = index.files_for(TileCoord::new(14, 2619, 6334).unwrap()).await.unwrap();

        let shown: Vec<_> = files.iter().map(|file| file.location.to_string()).collect();
        insta::assert_debug_snapshot!(shown, @r#"
        [
            "https://example.com/naip/1.tif",
            "https://example.com/naip/2.tif",
            "https://example.com/naip/3.tif",
            "https://example.com/naip/4.tif",
            "https://example.com/naip/5.tif",
            "https://example.com/naip/6.tif",
            "https://example.com/naip/7.tif",
            "https://example.com/naip/8.tif",
        ]
        "#);
    }
}
