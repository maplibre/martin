//! A vector `PMTiles` archive, built at test time.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};

use pmtiles::{PmTilesWriter, TileCoord, TileType};

use crate::{metadata, tiles};

type Metadata = BTreeMap<String, String>;

/// Repack the `mbtiles` fixture as a vector `PMTiles` archive at `dest`, and return that path.
///
/// The repo ships only raster `.pmtiles` fixtures, so the tests that need gzipped MVT tiles - what
/// a real vector archive holds - build one from an `.mbtiles` fixture, which stores its tiles the
/// same way. Tile bytes are carried over gzipped as they are, so what martin serves back is byte
/// for byte what the `.mbtiles` fixture holds.
pub async fn vector_pmtiles(mbtiles: impl AsRef<Path>, dest: impl AsRef<Path>) -> PathBuf {
    let mbtiles = mbtiles.as_ref();
    let dest = dest.as_ref().to_path_buf();
    let metadata = metadata(mbtiles).await;
    let bounds = numbers(&metadata, "bounds");
    let center = numbers(&metadata, "center");
    let file = File::create(&dest).expect("failed to create the pmtiles archive");
    let mut writer = PmTilesWriter::new(TileType::Mvt)
        .metadata(&pmtiles_metadata(&metadata))
        .min_zoom(zoom(&metadata, "minzoom"))
        .max_zoom(zoom(&metadata, "maxzoom"))
        .bounds(bounds[0], bounds[1], bounds[2], bounds[3])
        .center(center[0], center[1])
        .center_zoom(zoom(&metadata, "center"))
        .create(file)
        .expect("failed to write the pmtiles header");
    for (z, x, row, data) in tiles(mbtiles).await {
        let coord = coord(z, x, row);
        writer
            .add_raw_tile(coord, &data)
            .expect("failed to write a tile");
    }
    writer.finalize().expect("failed to finalize the archive");
    dest
}

/// The `PMTiles` coordinate of an `.mbtiles` row, which numbers rows from the south rather than
/// from the north.
fn coord(z: i64, x: i64, row: i64) -> TileCoord {
    let z = u8::try_from(z).expect("zoom is out of range");
    let x = u32::try_from(x).expect("column is out of range");
    let row = u32::try_from(row).expect("row is out of range");
    let y = (1u32 << z) - 1 - row;
    TileCoord::new(z, x, y).expect("tile is off the pyramid")
}

/// The `.mbtiles` metadata table as the single JSON object `PMTiles` stores instead.
///
/// The `json` row is itself an object holding `vector_layers`, which readers expect alongside the
/// other rows rather than nested, so it is merged in.
fn pmtiles_metadata(metadata: &Metadata) -> String {
    let mut object = serde_json::Map::new();
    for (key, value) in metadata {
        if key == "json" {
            let nested = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(value)
                .expect("the json metadata row is not a json object");
            object.extend(nested);
        } else {
            object.insert(key.clone(), value.as_str().into());
        }
    }
    serde_json::Value::Object(object).to_string()
}

/// The comma-separated numbers the `key` metadata row holds, such as `bounds` or `center`.
fn numbers(metadata: &Metadata, key: &str) -> Vec<f64> {
    metadata
        .get(key)
        .unwrap_or_else(|| panic!("the fixture has no {key} metadata"))
        .split(',')
        .map(|value| {
            value
                .parse()
                .unwrap_or_else(|e| panic!("malformed {key} metadata: {e}"))
        })
        .collect()
}

/// The zoom level the `key` metadata row ends with: `minzoom` and `maxzoom` hold only that, while
/// `center` trails it behind a longitude and a latitude.
fn zoom(metadata: &Metadata, key: &str) -> u8 {
    metadata
        .get(key)
        .unwrap_or_else(|| panic!("the fixture has no {key} metadata"))
        .rsplit(',')
        .next()
        .expect("rsplit yields at least one part")
        .parse()
        .unwrap_or_else(|e| panic!("malformed {key} metadata: {e}"))
}
