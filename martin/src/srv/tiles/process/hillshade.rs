//! The hillshade post-cache processor.

use std::num::NonZero;
use std::sync::LazyLock;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use martin_core::tiles::hillshade::{Canvas, bake_with_light};
use martin_core::tiles::neighbourhood::{
    InputEtag, NEIGHBOURHOOD_LEN, Neighbourhood, neighbourhood_etag,
};
use martin_core::tiles::{BoxedSource, MartinCoreError, Tile, TileCache};
use martin_tile_utils::{Encoding, TileCoord, TileInfo};
use tokio::sync::Semaphore;
use tracing::{debug, warn};

use super::ProcessError;
use super::neighbourhood::{GATHER_PERMITS, gather};
use crate::config::file::ResolvedHillshade;

/// Bounds concurrent bakes across the process, given that this is CPU bound.
static BAKE_PERMITS: LazyLock<Semaphore> = LazyLock::new(|| {
    let cores = std::thread::available_parallelism().map_or(4, NonZero::get);
    Semaphore::new(cores)
});

/// Bakes the hillshade tile for `xyz` from `source`'s normal tiles.
pub async fn bake_hillshade(
    source: &BoxedSource,
    settings: ResolvedHillshade,
    xyz: TileCoord,
    cache: Option<&TileCache>,
) -> Result<Tile, ProcessError> {
    let slots = {
        let _permit = GATHER_PERMITS
            .acquire()
            .await
            // The only way to fail is a closed semaphore, which happens at shutdown.
            .map_err(|_closed| ProcessError::HillshadeShuttingDown)?;
        gather(source, xyz, cache).await.map_err(|e| {
            if matches!(e.as_ref(), MartinCoreError::SourceNeedsReload) {
                ProcessError::HillshadeSourceNeedsReload
            } else {
                ProcessError::HillshadeSource(e.to_string())
            }
        })?
    };

    // An absent centre means the source has no data here at all.
    // Baking the clamped-from-nothing field would answer that with a flat tile, which a client cannot tell from real flat terrain.
    // A CDN would then hold for the life of its `max-age`.
    // An empty tile becomes a 204 downstream.
    //
    // Note this is *not* the corrupt-centre case, which `Canvas` rejects outright, nor a missing *neighbour*, which only degrades one seam.
    if slots[Neighbourhood::CENTRE].is_none() {
        debug!(
            source.id = source.get_id(),
            tile = %xyz,
            "No normal tile at this coordinate; serving no content rather than a blank bake"
        );
        return Ok(Tile::new_hash_etag(
            Vec::new(),
            TileInfo::new(settings.format, Encoding::Internal),
        ));
    }

    // Built before the bytes are moved into the bake.
    let etag = {
        let inputs: [InputEtag<'_>; NEIGHBOURHOOD_LEN] = std::array::from_fn(|i| {
            InputEtag::from_slot(slots[i].as_ref().map(|(_, etag)| etag.as_str()))
        });
        neighbourhood_etag(&inputs, &settings.fingerprint())
            .map(|hash| URL_SAFE_NO_PAD.encode(hash.to_ne_bytes()))
    };

    let neighbourhood = Neighbourhood::from_row_major(slots.map(|slot| slot.map(|(data, _)| data)));

    let format = settings.format;
    let encoded = {
        let _permit = BAKE_PERMITS
            .acquire()
            .await
            // The only way to fail is a closed semaphore, which happens at shutdown.
            .map_err(|_closed| ProcessError::HillshadeShuttingDown)?;
        // The bake is CPU-bound for tens of milliseconds, which would stall
        // every other task on this worker if it ran inline.
        tokio::task::spawn_blocking(move || {
            let canvas = Canvas::from_neighbourhood(&neighbourhood)?;
            let baked = bake_with_light(&canvas, 512, &settings.bake, settings.light.to_vector());
            baked.encode(format)
        })
        .await
        .map_err(|e| ProcessError::HillshadeBakeFailed(e.to_string()))??
    };

    let info = TileInfo::new(format, Encoding::Internal);
    if etag.is_none() {
        // An input could not be identified, so no honest tag can be derived.
        // Hashing the output bytes instead would produce a tag that looks
        // authoritative while saying nothing about whether the inputs changed.
        warn!(
            source.id = source.get_id(),
            tile = %xyz,
            "A normal tile carries no etag, so the baked hillshade is served without one"
        );
    }
    Ok(Tile::new_with_etag(encoded, info, etag.unwrap_or_default()))
}
