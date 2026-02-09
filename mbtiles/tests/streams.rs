use futures::{StreamExt as _, TryStreamExt as _};
use martin_tile_utils::{Tile, TileCoord};
use mbtiles::{MbtError, Mbtiles, create_metadata_table};
use sqlx::{Executor as _, SqliteConnection, query};

fn coord_key(coord: &TileCoord) -> (u8, u32, u32) {
    let TileCoord { z, x, y } = *coord;
    (z, x, y)
}

fn tile_key(tile: &Tile) -> (u8, u32, u32) {
    coord_key(&tile.0)
}

async fn new(rows: &[&str]) -> (Mbtiles, SqliteConnection) {
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

    for row in rows {
        let sql = format!(
            "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data)
            VALUES ({row});"
        );
        query(&sql).execute(&mut conn).await.expect(&sql);
    }

    (mbtiles, conn)
}

#[tokio::test(flavor = "current_thread")]
async fn mbtiles_stream_tiles() {
    let (mbtiles, mut conn) = new(&[
        // Note that `y`-coordinates are inverted.
        "1, 0, 1, CAST('tl' AS BLOB)",
        "1, 1, 0, CAST('br' AS BLOB)",
        "2, 0, 0, NULL",
    ])
    .await;

    {
        let mut coords: Vec<TileCoord> = mbtiles
            .stream_coords(&mut conn)
            .try_collect()
            .await
            .expect("Failed to collect tile coords");

        // Iteration order is not guaranteed.
        coords.sort_by_key(coord_key);

        assert_eq!(
            coords,
            [
                TileCoord { z: 1, x: 0, y: 0 },
                TileCoord { z: 1, x: 1, y: 1 },
                TileCoord { z: 2, x: 0, y: 3 },
            ]
        );
        // counter test: mbtiles must contain all tiles
        let mbt_type = mbtiles.detect_type(&mut conn).await.unwrap();
        for coord in coords {
            assert!(
                mbtiles
                    .contains(&mut conn, mbt_type, coord.z, coord.x, coord.y)
                    .await
                    .unwrap()
            );
        }
        assert!(
            !mbtiles
                .contains(&mut conn, mbt_type, 0, 0, 0)
                .await
                .unwrap()
        );
    }

    {
        let mut tiles: Vec<Tile> = mbtiles
            .stream_tiles(&mut conn)
            .try_collect()
            .await
            .expect("Failed to collect tiles");

        tiles.sort_by_key(tile_key);

        assert_eq!(
            tiles,
            [
                (TileCoord { z: 1, x: 0, y: 0 }, Some(b"tl".to_vec())),
                (TileCoord { z: 1, x: 1, y: 1 }, Some(b"br".to_vec())),
                (TileCoord { z: 2, x: 0, y: 3 }, None),
            ]
        );

        // counter test: mbtiles must contain all tiles
        let mbt_type = mbtiles.detect_type(&mut conn).await.unwrap();
        for (coord, _) in tiles {
            assert!(
                mbtiles
                    .contains(&mut conn, mbt_type, coord.z, coord.x, coord.y)
                    .await
                    .unwrap()
            );
        }
        assert!(
            !mbtiles
                .contains(&mut conn, mbt_type, 0, 0, 0)
                .await
                .unwrap()
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn mbtiles_stream_errors() {
    let (mbtiles, mut conn) = new(&[
        // Note that `y`-coordinates are inverted.
        // `4` is an invalid value for `x` at `z = 2`. A valid range is `0..=3`.
        "2, 4, 0, NULL",
    ])
    .await;

    {
        let mut stream = mbtiles.stream_coords(&mut conn);
        match stream.next().await {
            Some(Err(MbtError::InvalidTileIndex(..))) => {}
            _ => panic!("Unexpected value returned from stream!"),
        }
    }

    // Counter test: mbtiles must contain all tiles
    // the re-inverted y coordinate yielding 4 would be -1.
    // This is impossible to achieve without overflows.
    let mbt_type = mbtiles.detect_type(&mut conn).await.unwrap();
    for y in 0..=20 {
        assert!(
            !mbtiles
                .contains(&mut conn, mbt_type, 2, y, 0)
                .await
                .unwrap()
        );
    }
}
