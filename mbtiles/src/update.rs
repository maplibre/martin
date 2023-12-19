use log::info;
use sqlx::query;

use crate::errors::MbtResult;
use crate::Mbtiles;

impl Mbtiles {
    pub async fn update_metadata(&self) -> MbtResult<()> {
        let mut conn = self.open().await?;

        let info = query!(
            "
    SELECT min(zoom_level) AS min_zoom,
           max(zoom_level) AS max_zoom
    FROM tiles"
        )
        .fetch_one(&mut conn)
        .await?;

        if let Some(min_zoom) = info.min_zoom {
            info!("Updating minzoom to {min_zoom}");
            self.set_metadata_value(&mut conn, "minzoom", &min_zoom)
                .await?;
        }
        if let Some(max_zoom) = info.max_zoom {
            info!("Updating maxzoom to {max_zoom}");
            self.set_metadata_value(&mut conn, "maxzoom", &max_zoom)
                .await?;
        }

        Ok(())
    }
}
