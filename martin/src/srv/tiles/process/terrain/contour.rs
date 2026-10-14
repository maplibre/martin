//! The contour post-cache processor.

use std::num::NonZero;
use std::sync::LazyLock;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use compact_str::CompactString;
use martin_core::tiles::contour::{ContourError, trace_contours};
use martin_core::tiles::neighbourhood::{
    InputEtag, NEIGHBOURHOOD_LEN, Neighbourhood, neighbourhood_etag,
};
use martin_core::tiles::{BoxedSource, MartinCoreError, Tile, TileCache};
use martin_tile_utils::{Encoding, Format, TileCoord, TileInfo};
use tokio::sync::Semaphore;
use tracing::{debug, warn};

use super::neighbourhood::{GATHER_PERMITS, gather};
use crate::config::file::{ContourRangeError, ResolvedContour};

/// Errors that can occur while tracing a contour tile.
#[derive(thiserror::Error, Debug)]
pub enum ContourTraceError {
    /// A contour parameter supplied by the request was out of range.
    #[error(transparent)]
    Parameter(#[from] ContourRangeError),

    #[error("Contour tracing failed: {0}")]
    Core(#[from] ContourError),

    #[error("Could not read the elevation tiles a contour needs: {0}")]
    Source(String),

    /// The elevation source changed underneath us and must be reloaded before the trace can be retried.
    #[error("The elevation source changed and must be reloaded")]
    SourceNeedsReload,

    #[error("The contour trace did not complete: {0}")]
    TraceFailed(String),

    #[error("The server is shutting down and cannot start a new contour trace")]
    ShuttingDown,
}

impl From<ContourTraceError> for actix_web::Error {
    fn from(e: ContourTraceError) -> Self {
        match e {
            ContourTraceError::Parameter(ref inner) => {
                actix_web::error::ErrorBadRequest(inner.to_string())
            }
            ContourTraceError::ShuttingDown => {
                actix_web::error::ErrorServiceUnavailable(e.to_string())
            }
            ContourTraceError::Core(_)
            | ContourTraceError::Source(_)
            | ContourTraceError::SourceNeedsReload
            | ContourTraceError::TraceFailed(_) => {
                actix_web::error::ErrorInternalServerError(e.to_string())
            }
        }
    }
}

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
) -> Result<Tile, ContourTraceError> {
    let slots = {
        let _permit = GATHER_PERMITS
            .acquire()
            .await
            // The only way to fail is a closed semaphore, which happens at shutdown.
            .map_err(|_closed| ContourTraceError::ShuttingDown)?;
        gather(source, xyz, cache).await.map_err(|e| {
            if matches!(e.as_ref(), MartinCoreError::SourceNeedsReload) {
                ContourTraceError::SourceNeedsReload
            } else {
                ContourTraceError::Source(e.to_string())
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
        let inputs: [InputEtag<'_>; NEIGHBOURHOOD_LEN] =
            std::array::from_fn(|i| InputEtag::from_slot(slots[i].as_ref().map(|(_, etag)| etag)));
        neighbourhood_etag(&inputs, &settings.fingerprint())
            .map(|hash| URL_SAFE_NO_PAD.encode(hash.to_ne_bytes()))
            .map(CompactString::from)
    };

    let neighbourhood = Neighbourhood::from_row_major(slots.map(|slot| slot.map(|(data, _)| data)));

    let encoded = {
        let _permit = TRACE_PERMITS
            .acquire()
            .await
            // The only way to fail is a closed semaphore, which happens at shutdown.
            .map_err(|_closed| ContourTraceError::ShuttingDown)?;
        // Marching squares over a 320-square grid is CPU-bound for tens of
        // milliseconds, which would stall every other task on this worker if it
        // ran inline.
        tokio::task::spawn_blocking(move || trace_contours(&neighbourhood, xyz.z(), &settings.opts))
            .await
            .map_err(|e| ContourTraceError::TraceFailed(e.to_string()))??
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
