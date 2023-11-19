#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::str::FromStr;

use martin_tile_utils::{EARTH_CIRCUMFERENCE, EARTH_RADIUS};
use serde::Serialize;
use size_format::SizeFormatterBinary;
use sqlx::{query, SqliteExecutor};
use tilejson::Bounds;

use crate::{MbtResult, MbtType, Mbtiles};

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
            "|{:^9}|{:^9}|{:^9}|{:^9}|{:^9}| {:^20} |",
            "Zoom", "Count", "Smallest", "Largest", "Average", "BBox"
        )?;

        for l in &self.zoom_info {
            let min = SizeFormatterBinary::new(l.min_tile_size);
            let max = SizeFormatterBinary::new(l.max_tile_size);
            let avg = SizeFormatterBinary::new(l.avg_tile_size as u64);
            let prec = get_zoom_precision(l.zoom);

            writeln!(
                f,
                "|{:>9}|{:>9}|{:>9}|{:>9}|{:>9}| {:<20} |",
                l.zoom,
                l.tile_count,
                format!("{min:.2}B"),
                format!("{max:.2}B"),
                format!("{avg:.2}B"),
                format!("{:.prec$}", l.bbox),
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
                    "|{:>9}|{:>9}|{:>9}|{:>9}|{:>9}| {:<20} |",
                    "all",
                    self.tile_count,
                    format!("{min}B"),
                    format!("{max}B"),
                    format!("{avg}B"),
                    format!("{:.prec$}", bbox),
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
                        r.min_tile_x.unwrap(),
                        r.min_tile_y.unwrap(),
                        r.max_tile_x.unwrap(),
                        r.max_tile_y.unwrap(),
                    ),
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

/// Convert min/max XYZ tile coordinates to a bounding box
fn xyz_to_bbox(zoom: u8, min_x: i32, min_y: i32, max_x: i32, max_y: i32) -> Bounds {
    let tile_size = EARTH_CIRCUMFERENCE / f64::from(1_u32 << zoom);
    let (min_lng, min_lat) = webmercator_to_wgs84(
        -0.5 * EARTH_CIRCUMFERENCE + f64::from(min_x) * tile_size,
        -0.5 * EARTH_CIRCUMFERENCE + f64::from(min_y) * tile_size,
    );
    let (max_lng, max_lat) = webmercator_to_wgs84(
        -0.5 * EARTH_CIRCUMFERENCE + f64::from(max_x + 1) * tile_size,
        -0.5 * EARTH_CIRCUMFERENCE + f64::from(max_y + 1) * tile_size,
    );

    Bounds::new(min_lng, min_lat, max_lng, max_lat)
}

fn get_zoom_precision(zoom: u8) -> usize {
    let lng_delta = webmercator_to_wgs84(EARTH_CIRCUMFERENCE / f64::from(1_u32 << zoom), 0.0).0;
    let log = lng_delta.log10() - 0.5;
    if log > 0.0 {
        0
    } else {
        -log.ceil() as usize
    }
}

fn webmercator_to_wgs84(x: f64, y: f64) -> (f64, f64) {
    let lng = (x / EARTH_RADIUS).to_degrees();
    let lat = (f64::atan(f64::sinh(y / EARTH_RADIUS))).to_degrees();
    (lng, lat)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unreadable_literal)]

    use approx::assert_relative_eq;
    use insta::assert_yaml_snapshot;

    use crate::summary::webmercator_to_wgs84;
    use crate::{init_mbtiles_schema, MbtResult, MbtType, Mbtiles};

    #[actix_rt::test]
    async fn meter_to_lng_lat() {
        let (lng, lat) = webmercator_to_wgs84(-20037508.34, -20037508.34);
        assert_relative_eq!(lng, -179.99999991016847, epsilon = f64::EPSILON);
        assert_relative_eq!(lat, -85.05112877205713, epsilon = f64::EPSILON);

        let (lng, lat) = webmercator_to_wgs84(20037508.34, 20037508.34);
        assert_relative_eq!(lng, 179.99999991016847, epsilon = f64::EPSILON);
        assert_relative_eq!(lat, 85.05112877205713, epsilon = f64::EPSILON);

        let (lng, lat) = webmercator_to_wgs84(0.0, 0.0);
        assert_relative_eq!(lng, 0.0, epsilon = f64::EPSILON);
        assert_relative_eq!(lat, 0.0, epsilon = f64::EPSILON);

        let (lng, lat) = webmercator_to_wgs84(3000.0, 9000.0);
        assert_relative_eq!(lng, 0.02694945851388753, epsilon = f64::EPSILON);
        assert_relative_eq!(lat, 0.0808483487118794, epsilon = f64::EPSILON);
    }

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
          - -180
          - -85.0511287798066
          - 180
          - 85.0511287798066
        min_zoom: 0
        max_zoom: 6
        zoom_info:
          - zoom: 0
            tile_count: 1
            min_tile_size: 1107
            max_tile_size: 1107
            avg_tile_size: 1107
            bbox:
              - -180
              - -85.0511287798066
              - 180
              - 85.0511287798066
          - zoom: 1
            tile_count: 4
            min_tile_size: 160
            max_tile_size: 650
            avg_tile_size: 366.5
            bbox:
              - -180
              - -85.0511287798066
              - 180
              - 85.0511287798066
          - zoom: 2
            tile_count: 7
            min_tile_size: 137
            max_tile_size: 495
            avg_tile_size: 239.57142857142858
            bbox:
              - -180
              - -66.51326044311186
              - 180
              - 66.51326044311186
          - zoom: 3
            tile_count: 17
            min_tile_size: 67
            max_tile_size: 246
            avg_tile_size: 134
            bbox:
              - -135
              - -40.97989806962013
              - 180
              - 66.51326044311186
          - zoom: 4
            tile_count: 38
            min_tile_size: 64
            max_tile_size: 175
            avg_tile_size: 86
            bbox:
              - -135
              - -40.97989806962013
              - 180
              - 66.51326044311186
          - zoom: 5
            tile_count: 57
            min_tile_size: 64
            max_tile_size: 107
            avg_tile_size: 72.7719298245614
            bbox:
              - -123.75000000000001
              - -40.97989806962013
              - 180
              - 61.60639637138627
          - zoom: 6
            tile_count: 72
            min_tile_size: 64
            max_tile_size: 97
            avg_tile_size: 68.29166666666667
            bbox:
              - -123.75000000000001
              - -40.97989806962013
              - 180
              - 61.60639637138627
        "###);

        Ok(())
    }
}
