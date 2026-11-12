//! Phase 2, merge, encode and write: encodes the tiles the sort groups, on every thread.

mod assemble;
mod columns;
mod encode;
mod per_tile;

use martin_tile_utils::{Encoding, Format};
use mlt_core::PropKind;
use tilejson::{TileJSON, VectorLayer};

pub use crate::phase2::encode::TileFormat;
use crate::phase2::encode::{DedupIndex, EncodeSettings, LayerStats};
pub(crate) use crate::phase2::encode::{LayerInfo, TileEncoder};
pub(crate) use crate::phase2::per_tile::LayerOpCounts;
use crate::phase2::per_tile::OutputExprs;
pub use crate::phase2::per_tile::{OpCounts, OpKind};
use crate::plan::Plan;
use crate::props::KeyNames;
use crate::sort::{TileBatches, TileRecords};
use crate::{EncodedTile, TileGenResult, TileOrder};

/// The stage types of phase 2, for this crate's benches and stage-level tests only.
#[cfg(feature = "internals")]
pub(crate) mod internals {
    pub use crate::phase2::assemble::{AssembledLayer, LayerAssembler};
    pub use crate::phase2::encode::{DedupIndex, EncodeSettings, LayerInfo, LayerStats, TileEncoder};
    pub use crate::phase2::per_tile::TileOps;
}

/// Encodes grouped tiles of a rendered plan, shared by every encoder thread.
pub(crate) struct TileEncoding<'p> {
    infos: Vec<LayerInfo>,
    /// Only complete once every feature is rendered, since rendering interns new keys.
    names: Vec<KeyNames>,
    format: TileFormat,
    encoding: Encoding,
    order: TileOrder,
    mvt_keep_unused_keys: bool,
    dedup: DedupIndex,
    exprs: OutputExprs<'p>,
}

impl<'p> TileEncoding<'p> {
    /// The encoding of `plan`'s tiles, handing `batches` back. Taking the batches means their sorter
    /// was consumed, so every render worker is done and every property key has a name.
    pub(crate) fn start(
        plan: &'p Plan,
        batches: TileBatches,
        format: TileFormat,
        encoding: Encoding,
        order: TileOrder,
        mvt_keep_unused_keys: bool,
    ) -> (Self, TileBatches) {
        let names = plan.key_names();
        let encoding = Self {
            infos: plan.layers.iter().map(|layer| layer.info.clone()).collect(),
            exprs: OutputExprs::new(plan, &names),
            names,
            format,
            encoding,
            order,
            mvt_keep_unused_keys,
            dedup: DedupIndex::default(),
        };
        (encoding, batches)
    }

    /// Encodes `batch` with `encoder`, which keeps the layer stats of every tile it encodes.
    pub(crate) fn encode(&self, encoder: &mut TileEncoder, batch: Vec<TileRecords>) -> TileGenResult<Vec<EncodedTile>> {
        let settings = EncodeSettings {
            layers: &self.infos,
            keys: &self.names,
            format: self.format,
            encoding: self.encoding,
            order: self.order,
            mvt_keep_unused_keys: self.mvt_keep_unused_keys,
            exprs: Some(&self.exprs),
        };
        encoder.encode_batch(&settings, &self.dedup, batch)
    }

    /// Fills `metadata` with the zooms, format and `vector_layers` that `encoders` produced.
    pub(crate) fn complete_metadata(&self, metadata: &mut TileJSON, encoders: &[TileEncoder]) {
        let mut stats = vec![LayerStats::default(); self.infos.len()];
        for encoder in encoders {
            for (total, layer) in stats.iter_mut().zip(&encoder.stats) {
                total.merge(layer);
            }
        }
        let zooms = stats.iter().filter_map(|s| s.zooms);
        metadata.minzoom = zooms.clone().map(|z| z.0).min();
        metadata.maxzoom = zooms.map(|z| z.1).max();
        let format = Format::from(self.format).metadata_format_value();
        metadata.other.insert("format".to_owned(), format.into());
        metadata.vector_layers = Some(
            self.infos
                .iter()
                .zip(&stats)
                .filter(|(_, stats)| stats.zooms.is_some())
                .map(|(info, stats)| {
                    let fields = stats
                        .fields
                        .iter()
                        .map(|(name, &kind)| (name.clone(), field_type(kind).to_owned()));
                    let mut layer = VectorLayer::new(info.name.clone(), fields.collect());
                    (layer.minzoom, layer.maxzoom) = stats.zooms.unzip();
                    layer
                })
                .collect(),
        );
    }
}

/// `vector_layers` field types, as `MBTiles` and `TileJSON` consumers expect them.
fn field_type(kind: PropKind) -> &'static str {
    match kind {
        PropKind::Bool => "Boolean",
        PropKind::Str => "String",
        PropKind::I8
        | PropKind::U8
        | PropKind::I32
        | PropKind::U32
        | PropKind::I64
        | PropKind::U64
        | PropKind::F32
        | PropKind::F64 => "Number",
    }
}
