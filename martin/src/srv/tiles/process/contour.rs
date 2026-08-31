//! The contour post-cache processor.

use std::num::NonZero;
use std::sync::LazyLock;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use martin_core::tiles::contour::trace_contours;
use martin_core::tiles::neighbourhood::{
    InputEtag, NEIGHBOURHOOD_LEN, Neighbourhood, neighbourhood_etag,
};
use martin_core::tiles::{BoxedSource, MartinCoreError, Tile, TileCache};
use martin_tile_utils::{Encoding, Format, TileCoord, TileInfo};
use tokio::sync::Semaphore;
use tracing::{debug, warn};

use super::ProcessError;
use super::neighbourhood::{GATHER_PERMITS, gather};
use crate::config::file::ResolvedContour;

/// Bounds concurrent traces across the process, given that this is CPU bound.
static TRACE_PERMITS: LazyLock<Semaphore> = LazyLock::new(|| {
    let cores = std::thread::available_parallelism().map_or(4, NonZero::get);
    Semaphore::new(cores)
});

/// Traces the contour tile for `xyz` from `source`'s Terrarium elevation tiles.
pub async fn trace_contour(
    source: &BoxedSource,
    settings: ResolvedContour,
    xyz: TileCoord,
    cache: Option<&TileCache>,
) -> Result<Tile, ProcessError> {
    let slots = {
        let _permit = GATHER_PERMITS
            .acquire()
            .await
            // The only way to fail is a closed semaphore, which happens at shutdown.
            .map_err(|_closed| ProcessError::ContourShuttingDown)?;
        gather(source, xyz, cache).await.map_err(|e| {
            if matches!(e.as_ref(), MartinCoreError::SourceNeedsReload) {
                ProcessError::ContourSourceNeedsReload
            } else {
                ProcessError::ContourSource(e.to_string())
            }
        })?
    };

    let info = TileInfo::new(Format::Mvt, Encoding::Uncompressed);

    // An absent centre means the source has no elevation here at all. Tracing the
    // clamped-from-nothing field would answer with a tile a client cannot tell
    // from genuinely contour-free terrain, which a CDN would then hold for the
    // life of its `max-age`. An empty tile becomes a 204 downstream.
    //
    // Note this is *not* the corrupt-centre case, which the assembler rejects
    // outright, nor a missing *neighbour*, which only degrades one seam.
    if slots[Neighbourhood::CENTRE].is_none() {
        debug!(
            source.id = source.get_id(),
            tile = %xyz,
            "No elevation tile at this coordinate; serving no content rather than an empty trace"
        );
        return Ok(Tile::new_hash_etag(Vec::new(), info));
    }

    // Built before the bytes are moved into the trace.
    let etag = {
        let inputs: [InputEtag<'_>; NEIGHBOURHOOD_LEN] = std::array::from_fn(|i| {
            InputEtag::from_slot(slots[i].as_ref().map(|(_, etag)| etag.as_str()))
        });
        neighbourhood_etag(&inputs, &settings.fingerprint())
            .map(|hash| URL_SAFE_NO_PAD.encode(hash.to_ne_bytes()))
    };

    let neighbourhood = Neighbourhood::from_row_major(slots.map(|slot| slot.map(|(data, _)| data)));

    let encoded = {
        let _permit = TRACE_PERMITS
            .acquire()
            .await
            // The only way to fail is a closed semaphore, which happens at shutdown.
            .map_err(|_closed| ProcessError::ContourShuttingDown)?;
        // Marching squares over a 320-square grid is CPU-bound for tens of
        // milliseconds, which would stall every other task on this worker if it
        // ran inline.
        tokio::task::spawn_blocking(move || trace_contours(&neighbourhood, xyz.z, &settings.opts))
            .await
            .map_err(|e| ProcessError::ContourTraceFailed(e.to_string()))??
    };

    if etag.is_none() {
        // An input could not be identified, so no honest tag can be derived.
        // Hashing the output bytes instead would produce a tag that looks
        // authoritative while saying nothing about whether the inputs changed.
        warn!(
            source.id = source.get_id(),
            tile = %xyz,
            "An elevation tile carries no etag, so the traced contours are served without one"
        );
    }
    Ok(Tile::new_with_etag(encoded, info, etag.unwrap_or_default()))
}
