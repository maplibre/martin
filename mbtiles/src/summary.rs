#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::str::FromStr;

use martin_tile_utils::{get_zoom_precision, xyz_to_bbox};
use serde::Serialize;
use size_format::SizeFormatterBinary;
use sqlx::{query, SqliteExecutor};
use tilejson::Bounds;

use crate::{invert_y_value, MbtResult, MbtType, Mbtiles};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ZoomInfo {
    pub zoom: u8,
    pub tile_count: u64,
    pub min_tile_size: u64,
    pub max_tile_size: u64,
    pub avg_tile_size: f64,
    pub bbox: Bounds,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Summary {
    pub file_size: Option<u64>,
    pub mbt_type: MbtType,
    pub page_size: u64,
    pub page_count: u64,
    pub tile_count: u64,
    pub min_tile_size: Option<u64>,
    pub max_tile_size: Option<u64>,
    pub avg_tile_size: f64,
    pub bbox: Option<Bounds>,
    pub min_zoom: Option<u8>,
    pub max_zoom: Option<u8>,
    pub zoom_info: Vec<ZoomInfo>,
}

impl Display for Summary {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Schema: {}", self.mbt_type)?;

        if let Some(file_size) = self.file_size {
            let file_size = SizeFormatterBinary::new(file_size);
            writeln!(f, "File size: {file_size:.2}B")?;
        } else {
            writeln!(f, "File size: unknown")?;
        }
        let page_size = SizeFormatterBinary::new(self.page_size);
        writeln!(f, "Page size: {page_size:.2}B")?;
        writeln!(f, "Page count: {:.2}", self.page_count)?;
        writeln!(f)?;
        writeln!(
            f,
            " {:^4} | {:^9} | {:^9} | {:^9} | {:^9} | Bounding Box",
            "Zoom", "Count", "Smallest", "Largest", "Average"
        )?;

        for l in &self.zoom_info {
            let min = SizeFormatterBinary::new(l.min_tile_size);
            let max = SizeFormatterBinary::new(l.max_tile_size);
            let avg = SizeFormatterBinary::new(l.avg_tile_size as u64);
            let prec = get_zoom_precision(l.zoom);

            writeln!(
                f,
                " {:>4} | {:>9} | {:>9} | {:>9} | {:>9} | {:.prec$}",
                l.zoom,
                l.tile_count,
                format!("{min:.1}B"),
                format!("{max:.1}B"),
                format!("{avg:.1}B"),
                l.bbox,
            )?;
        }

        if self.zoom_info.len() > 1 {
            if let (Some(min), Some(max), Some(bbox), Some(max_zoom)) = (
                self.min_tile_size,
                self.max_tile_size,
                self.bbox,
                self.max_zoom,
            ) {
                let min = SizeFormatterBinary::new(min);
                let max = SizeFormatterBinary::new(max);
                let avg = SizeFormatterBinary::new(self.avg_tile_size as u64);
                let prec = get_zoom_precision(max_zoom);
                writeln!(
                    f,
                    " {:>4} | {:>9} | {:>9} | {:>9} | {:>9} | {bbox:.prec$}",
                    "all",
                    self.tile_count,
                    format!("{min}B"),
                    format!("{max}B"),
                    format!("{avg}B"),
                )?;
            }
        }

        Ok(())
    }
}

impl Mbtiles {
    /// Compute `MBTiles` file summary
    pub async fn summary<T>(&self, conn: &mut T) -> MbtResult<Summary>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let mbt_type = self.detect_type(&mut *conn).await?;
        let file_size = PathBuf::from_str(self.filepath())
            .ok()
            .and_then(|p| p.metadata().ok())
            .map(|m| m.len());

        let sql = query!("PRAGMA page_size;");
        let page_size = sql.fetch_one(&mut *conn).await?.page_size.unwrap() as u64;

        let sql = query!("PRAGMA page_count;");
        let page_count = sql.fetch_one(&mut *conn).await?.page_count.unwrap() as u64;

        let zoom_info = query!(
            "
    SELECT zoom_level             AS zoom,
           count()                AS count,
           min(length(tile_data)) AS smallest,
           max(length(tile_data)) AS largest,
           avg(length(tile_data)) AS average,
           min(tile_column)       AS min_tile_x,
           min(tile_row)          AS min_tile_y,
           max(tile_column)       AS max_tile_x,
           max(tile_row)          AS max_tile_y
    FROM tiles
    GROUP BY zoom_level"
        )
        .fetch_all(&mut *conn)
        .await?;

        let zoom_info: Vec<ZoomInfo> = zoom_info
            .into_iter()
            .map(|r| {
                let zoom = u8::try_from(r.zoom.unwrap()).expect("zoom_level is not a u8");
                ZoomInfo {
                    zoom,
                    tile_count: r.count as u64,
                    min_tile_size: r.smallest.unwrap_or(0) as u64,
                    max_tile_size: r.largest.unwrap_or(0) as u64,
                    avg_tile_size: r.average.unwrap_or(0.0),
                    bbox: xyz_to_bbox(
                        zoom,
                        r.min_tile_x.unwrap() as u32,
                        invert_y_value(zoom, r.max_tile_y.unwrap() as u32),
                        r.max_tile_x.unwrap() as u32,
                        invert_y_value(zoom, r.min_tile_y.unwrap() as u32),
                    )
                    .into(),
                }
            })
            .collect();

        let tile_count = zoom_info.iter().map(|l| l.tile_count).sum();
        let avg_sum = zoom_info
            .iter()
            .map(|l| l.avg_tile_size * l.tile_count as f64)
            .sum::<f64>();

        Ok(Summary {
            file_size,
            mbt_type,
            page_size,
            page_count,
            tile_count,
            min_tile_size: zoom_info.iter().map(|l| l.min_tile_size).reduce(u64::min),
            max_tile_size: zoom_info.iter().map(|l| l.max_tile_size).reduce(u64::max),
            avg_tile_size: avg_sum / tile_count as f64,
            bbox: zoom_info.iter().map(|l| l.bbox).reduce(|a, b| a + b),
            min_zoom: zoom_info.iter().map(|l| l.zoom).reduce(u8::min),
            max_zoom: zoom_info.iter().map(|l| l.zoom).reduce(u8::max),
            zoom_info,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unreadable_literal)]

    use insta::assert_yaml_snapshot;

    use crate::{init_mbtiles_schema, MbtResult, MbtType, Mbtiles};

    #[actix_rt::test]
    async fn summary_empty_file() -> MbtResult<()> {
        let mbt = Mbtiles::new("file:mbtiles_empty_summary?mode=memory&cache=shared")?;
        let mut conn = mbt.open().await?;

        init_mbtiles_schema(&mut conn, MbtType::Flat).await.unwrap();
        let res = mbt.summary(&mut conn).await?;
        assert_yaml_snapshot!(res, @r###"
        ---
        file_size: ~
        mbt_type: Flat
        page_size: 512
        page_count: 6
        tile_count: 0
        min_tile_size: ~
        max_tile_size: ~
        avg_tile_size: NaN
        bbox: ~
        min_zoom: ~
        max_zoom: ~
        zoom_info: []
        "###);

        Ok(())
    }

    #[actix_rt::test]
    async fn summary() -> MbtResult<()> {
        let mbt = Mbtiles::new("../tests/fixtures/mbtiles/world_cities.mbtiles")?;
        let mut conn = mbt.open().await?;

        let res = mbt.summary(&mut conn).await?;

        assert_yaml_snapshot!(res, @r###"
        ---
        file_size: 49152
        mbt_type: Flat
        page_size: 4096
        page_count: 12
        tile_count: 196
        min_tile_size: 64
        max_tile_size: 1107
        avg_tile_size: 96.2295918367347
        bbox:
          - -179.99999999999955
          - -85.05112877980659
          - 180.00000000000028
          - 85.05112877980655
        min_zoom: 0
        max_zoom: 6
        zoom_info:
          - zoom: 0
            tile_count: 1
            min_tile_size: 1107
            max_tile_size: 1107
            avg_tile_size: 1107
            bbox:
              - -179.99999999999955
              - -85.05112877980659
              - 179.99999999999986
              - 85.05112877980655
          - zoom: 1
            tile_count: 4
            min_tile_size: 160
            max_tile_size: 650
            avg_tile_size: 366.5
            bbox:
              - -179.99999999999955
              - -85.05112877980652
              - 179.99999999999915
              - 85.05112877980655
          - zoom: 2
            tile_count: 7
            min_tile_size: 137
            max_tile_size: 495
            avg_tile_size: 239.57142857142858
            bbox:
              - -179.99999999999955
              - -66.51326044311165
              - 179.99999999999915
              - 66.51326044311182
          - zoom: 3
            tile_count: 17
            min_tile_size: 67
            max_tile_size: 246
            avg_tile_size: 134
            bbox:
              - -134.99999999999957
              - -40.979898069620376
              - 180.00000000000028
              - 66.51326044311169
          - zoom: 4
            tile_count: 38
            min_tile_size: 64
            max_tile_size: 175
            avg_tile_size: 86
            bbox:
              - -134.99999999999963
              - -40.979898069620106
              - 179.99999999999966
              - 66.51326044311175
          - zoom: 5
            tile_count: 57
            min_tile_size: 64
            max_tile_size: 107
            avg_tile_size: 72.7719298245614
            bbox:
              - -123.74999999999966
              - -40.979898069620106
              - 179.99999999999966
              - 61.606396371386154
          - zoom: 6
            tile_count: 72
            min_tile_size: 64
            max_tile_size: 97
            avg_tile_size: 68.29166666666667
            bbox:
              - -123.74999999999957
              - -40.979898069620305
              - 180.00000000000009
              - 61.606396371386104
        "###);

        Ok(())
    }
}
