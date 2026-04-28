use enum_display::EnumDisplay;
use log::{debug, info, warn};
use martin_tile_utils::TileInfo;
use sqlx::{SqliteExecutor, query};

use self::UpdateZoomType::{GrowOnly, Reset, Skip};
use crate::MbtError::InvalidZoomValue;
use crate::errors::MbtResult;
use crate::{Mbtiles, compute_min_max_zoom};

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, EnumDisplay)]
#[enum_display(case = "Kebab")]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum UpdateZoomType {
    /// Reset the minzoom and maxzoom metadata values to match the content of the tiles table
    #[default]
    Reset,
    /// Only update minzoom and maxzoom if the zooms in the tiles table are outside the range set in the metadata
    GrowOnly,
    /// Perform a dry run and print result, without updating the minzoom and maxzoom metadata values
    Skip,
}

impl Mbtiles {
    async fn set_zoom_value<T>(
        &self,
        conn: &mut T,
        is_max_zoom: bool,
        calc_zoom: u8,
        update_zoom: UpdateZoomType,
    ) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let zoom_name = if is_max_zoom { "maxzoom" } else { "minzoom" };
        match self.get_metadata_zoom_value(conn, zoom_name).await {
            Ok(Some(meta_zoom)) => {
                let is_outside_range = if is_max_zoom {
                    meta_zoom < calc_zoom
                } else {
                    meta_zoom > calc_zoom
                };
                if meta_zoom == calc_zoom {
                    info!("Metadata value {zoom_name} is already set to correct value {meta_zoom}");
                } else if update_zoom == Skip {
                    info!(
                        "Metadata value {zoom_name} is set to {meta_zoom}, but should be set to {calc_zoom}. Skipping update"
                    );
                } else if is_outside_range || update_zoom == Reset {
                    info!("Updating metadata {zoom_name} from {meta_zoom} to {calc_zoom}");
                    self.set_metadata_value(conn, zoom_name, calc_zoom).await?;
                } else if is_max_zoom {
                    info!(
                        "Metadata value {zoom_name}={meta_zoom} is greater than the computed {zoom_name} {calc_zoom} in tiles table, not updating"
                    );
                } else {
                    info!(
                        "Metadata value {zoom_name}={meta_zoom} is less than the computed {zoom_name} {calc_zoom} in tiles table, not updating"
                    );
                }
            }
            Ok(None) => {
                info!("Setting metadata value {zoom_name} to {calc_zoom}");
                self.set_metadata_value(conn, zoom_name, calc_zoom).await?;
            }
            Err(InvalidZoomValue(_, val)) => {
                warn!("Overriding invalid metadata value {zoom_name}='{val}' to {calc_zoom}");
                self.set_metadata_value(conn, zoom_name, calc_zoom).await?;
            }
            Err(e) => Err(e)?,
        }
        Ok(())
    }

    /// Samples a tile to detect the encoding and updates the `compression` metadata key.
    /// For uncompressed or internally-compressed formats (e.g. PNG/JPEG), the key is
    /// *removed* from the metadata table rather than being set to `"none"`.
    pub async fn update_compression<T>(&self, conn: &mut T) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let row = query!("SELECT tile_data FROM tiles WHERE tile_data IS NOT NULL LIMIT 1")
            .fetch_optional(&mut *conn)
            .await?;

        if let Some(r) = row {
            if let Some(tile_data) = r.tile_data {
                let tile_info = TileInfo::detect(&tile_data);
                debug!("Detected tile info for compression update: {tile_info}");
                if let Some(compression) = tile_info.encoding.compression() {
                    info!("Setting metadata compression to '{compression}'");
                    self.set_metadata_value(conn, "compression", compression)
                        .await?;
                } else {
                    info!(
                        "Tiles use no external compression; removing 'compression' from metadata if present"
                    );
                    self.delete_metadata_value(conn, "compression").await?;
                }
            }
        } else {
            debug!("No tiles found, skipping compression metadata update");
        }

        Ok(())
    }

    /// Update the metadata table with the min and max zoom levels
    /// from the tiles table.
    /// If `grow_only` is true, only update the metadata if the
    /// new min or max zoom is outside the current range.
    #[hotpath::measure]
    pub async fn update_metadata<T>(
        &self,
        conn: &mut T,
        update_zoom: UpdateZoomType,
    ) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        match (update_zoom, compute_min_max_zoom(&mut *conn).await?) {
            (_, Some((min_zoom, max_zoom))) => {
                self.set_zoom_value(&mut *conn, false, min_zoom, update_zoom)
                    .await?;
                self.set_zoom_value(&mut *conn, true, max_zoom, update_zoom)
                    .await?;
            }
            (GrowOnly | Skip, None) => {
                info!("No tiles found in the tiles table, skipping metadata min/max zoom update");
            }
            (Reset, None) => {
                info!("No tiles found in the tiles table, deleting minzoom and maxzoom if exist");
                self.delete_metadata_value(&mut *conn, "minzoom").await?;
                self.delete_metadata_value(&mut *conn, "maxzoom").await?;
            }
        }

        self.update_compression(&mut *conn).await?;

        Ok(())
    }
}
