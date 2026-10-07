//! Encodes tiles: assembles each layer, writes MLT or MVT, compresses, and tags likely duplicates.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, PoisonError};

use martin_tile_utils::Encoding;
use mlt_core::PropKind;
use mlt_core::encoder::EncoderConfig;
use mlt_core::mvt::tile_layers_to_mvt;
use xxhash_rust::xxh3::xxh3_64;

use crate::group::TileRecords;
use crate::props::{KeyNames, widen};
use crate::{EncodedTile, LayerAssembler, LayerGrid, Seq, TileGenResult, TileOrder};

#[derive(Clone, Copy, Debug)]
pub enum TileFormat {
    Mlt(EncoderConfig),
    Mvt,
}

#[derive(Clone, Debug)]
pub struct LayerInfo {
    pub name: String,
    pub grid: LayerGrid,
    pub order: FeatureOrder,
}

/// How a layer's features are ordered inside a tile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FeatureOrder {
    /// Source order, i.e. draw order. Also the cheapest: the MLT encoder tries no other orders.
    #[default]
    Source,
    /// MLT may sort by feature id when that is smaller.
    Id,
    /// MLT tries its sort orders and keeps the smallest result.
    Auto,
}

impl FeatureOrder {
    fn apply(self, config: EncoderConfig) -> EncoderConfig {
        let spatial = self == Self::Auto;
        config
            .with_id_sort(self != Self::Source && config.attempt_id_sort())
            .with_spatial_morton_sort(spatial && config.attempt_spatial_morton_sort())
            .with_spatial_hilbert_sort(spatial && config.attempt_spatial_hilbert_sort())
    }
}

/// Everything that is the same for every tile of a run.
pub struct EncodeSettings<'a> {
    /// Indexed by the layer byte of the sort key; `keys` is parallel to it.
    pub layers: &'a [LayerInfo],
    pub keys: &'a [KeyNames],
    pub format: TileFormat,
    pub encoding: Encoding,
    pub order: TileOrder,
}

/// Dedup keys for tiles likely to repeat, shared by all encoders. A key is only handed out for bytes
/// equal to the first tile that produced its hash, so sinks can trust equal keys without comparing.
#[derive(Default)]
pub struct DedupIndex {
    tiles: Mutex<HashMap<u64, Vec<u8>>>,
}

impl DedupIndex {
    fn key(&self, data: &[u8]) -> Option<u64> {
        let hash = xxh3_64(data);
        match self
            .tiles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(hash)
        {
            Entry::Vacant(slot) => {
                slot.insert(data.to_vec());
                Some(hash)
            }
            Entry::Occupied(known) => (known.get() == data).then_some(hash),
        }
    }
}

/// What a layer's tiles held, for the tileset's `vector_layers`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerStats {
    pub zooms: Option<(u8, u8)>,
    pub features: u64,
    pub fields: BTreeMap<String, PropKind>,
}

impl LayerStats {
    fn add_features(&mut self, zoom: u8, features: u64) {
        self.zooms = Some(
            self.zooms
                .map_or((zoom, zoom), |(lo, hi)| (lo.min(zoom), hi.max(zoom))),
        );
        self.features += features;
    }

    fn add_field(&mut self, name: &str, kind: PropKind) {
        if let Some(known) = self.fields.get_mut(name) {
            *known = widen(*known, kind);
        } else {
            self.fields.insert(name.to_owned(), kind);
        }
    }

    pub fn merge(&mut self, other: &Self) {
        if let Some((lo, hi)) = other.zooms {
            self.zooms = Some(self.zooms.map_or((lo, hi), |(a, b)| (a.min(lo), b.max(hi))));
        }
        self.features += other.features;
        for (name, &kind) in &other.fields {
            self.add_field(name, kind);
        }
    }
}

/// One per encoder thread: scratch buffers, the last tile for reuse, and statistics per layer.
#[derive(Default)]
pub struct TileEncoder {
    assembler: LayerAssembler,
    previous: Option<Previous>,
    pub stats: Vec<LayerStats>,
}

struct Previous {
    records: TileRecords,
    tile: EncodedTile,
    /// Features per layer, so a reused tile still counts in the statistics of its own zoom.
    features: Vec<(u8, u64)>,
}

impl TileEncoder {
    pub fn encode_batch(
        &mut self,
        settings: &EncodeSettings<'_>,
        dedup: &DedupIndex,
        batch: Vec<TileRecords>,
    ) -> TileGenResult<Vec<EncodedTile>> {
        if self.stats.len() < settings.layers.len() {
            self.stats
                .resize_with(settings.layers.len(), LayerStats::default);
        }
        let mut out = Vec::with_capacity(batch.len());
        for records in batch {
            let coord = settings.order.tile_coord(records.tile_id)?;
            let previous = match self.previous.take() {
                // Polygon interiors and repeated features make runs of identical tiles; encode them once.
                Some(mut previous) if previous.records.same_content(&records) => {
                    if previous.tile.dedup.is_none() {
                        previous.tile.dedup = dedup.key(&previous.tile.data);
                    }
                    for &(layer, features) in &previous.features {
                        self.stats[usize::from(layer)].add_features(coord.z(), features);
                    }
                    Previous {
                        records,
                        tile: EncodedTile {
                            coord,
                            ..previous.tile
                        },
                        features: previous.features,
                    }
                }
                _ => {
                    let mut features = Vec::new();
                    let data = self.encode_tile(settings, &records, coord.z(), &mut features)?;
                    let key = if records.is_fill_only() {
                        dedup.key(&data)
                    } else {
                        None
                    };
                    Previous {
                        records,
                        tile: EncodedTile {
                            coord,
                            data,
                            dedup: key,
                        },
                        features,
                    }
                }
            };
            out.push(previous.tile.clone());
            self.previous = Some(previous);
        }
        Ok(out)
    }

    fn encode_tile(
        &mut self,
        settings: &EncodeSettings<'_>,
        tile: &TileRecords,
        zoom: u8,
        features: &mut Vec<(u8, u64)>,
    ) -> TileGenResult<Vec<u8>> {
        let mut layers = Vec::new();
        let mut orders = Vec::new();
        let mut records: Vec<(Seq, &[u8])> = Vec::new();
        let mut all = tile.records().peekable();
        while let Some(&(layer, _, _)) = all.peek() {
            records.clear();
            while let Some((_, seq, bytes)) = all.next_if(|&(l, _, _)| l == layer) {
                records.push((seq, bytes));
            }
            let info = &settings.layers[usize::from(layer)];
            let assembled = self.assembler.assemble(
                &info.name,
                info.grid,
                &settings.keys[usize::from(layer)],
                &records,
            )?;
            let stats = &mut self.stats[usize::from(layer)];
            let count = assembled.features().len() as u64;
            stats.add_features(zoom, count);
            for (name, &kind) in assembled
                .property_names()
                .iter()
                .zip(assembled.property_kinds())
            {
                stats.add_field(name, kind);
            }
            features.push((layer, count));
            layers.push(assembled);
            orders.push(info.order);
        }
        let data = match settings.format {
            TileFormat::Mlt(config) => {
                let mut data = Vec::new();
                for (layer, order) in layers.into_iter().zip(orders) {
                    data.extend(layer.encode(order.apply(config))?);
                }
                data
            }
            TileFormat::Mvt => tile_layers_to_mvt(layers)?,
        };
        Ok(martin_tile_utils::encode(data, settings.encoding)?)
    }
}

#[cfg(test)]
mod tests {
    use martin_tile_utils::{TileCoord, decode_gzip};
    use mlt_core::{Decoder, Parser};

    use super::*;
    use crate::props::{KeyInterner, PropRef};
    use crate::record::{EncodedProps, Geom, encode};

    fn tile(tile_id: u64, records: &[(u8, u64, Geom<'_>)], interner: &KeyInterner) -> TileRecords {
        let mut tile = TileRecords::new(tile_id);
        for &(layer, row, geom) in records {
            let mut props = EncodedProps::default();
            props.push(interner.intern("kind"), PropRef::Str("park"));
            let mut bytes = Vec::new();
            encode(&mut bytes, Some(row), &props, geom);
            tile.push_record(layer, Seq::new(0, row).unwrap(), &bytes);
        }
        tile
    }

    fn settings<'a>(
        layers: &'a [LayerInfo],
        keys: &'a [KeyNames],
        format: TileFormat,
    ) -> EncodeSettings<'a> {
        EncodeSettings {
            layers,
            keys,
            format,
            encoding: Encoding::Gzip,
            order: TileOrder::Tms,
        }
    }

    fn layers() -> Vec<LayerInfo> {
        ["parks", "roads"]
            .map(|name| LayerInfo {
                name: name.to_owned(),
                grid: LayerGrid {
                    extent: 4096,
                    buffer: 64,
                },
                order: FeatureOrder::Source,
            })
            .to_vec()
    }

    #[test]
    fn encodes_layers_in_order_and_dedups_fill_runs() {
        let interner = [KeyInterner::new(["kind"]), KeyInterner::new(["kind"])];
        let fill = |id| tile(id, &[(0, 1, Geom::Fill)], &interner[0]);
        let batch = vec![
            tile(
                1,
                &[
                    (0, 1, Geom::Fill),
                    (
                        1,
                        2,
                        Geom::Lines {
                            parts: &[2],
                            vertices: &[[0, 0], [9, 9]],
                        },
                    ),
                ],
                &interner[0],
            ),
            fill(2),
            fill(3),
        ];
        let keys = interner.map(KeyInterner::freeze);
        let layers = layers();
        let settings = settings(&layers, &keys, TileFormat::Mlt(EncoderConfig::default()));
        let mut encoder = TileEncoder::default();
        let tiles = encoder
            .encode_batch(&settings, &DedupIndex::default(), batch)
            .unwrap();

        assert_eq!(
            tiles[0].coord,
            TileCoord::new_unchecked(1, 0, 1),
            "TMS id 1 is z1 x0, TMS row 0"
        );
        let raw = decode_gzip(&tiles[0].data).unwrap();
        let names: Vec<_> = Parser::default()
            .parse_layers(&raw)
            .unwrap()
            .into_iter()
            .map(|l| {
                l.into_tile(&mut Decoder::default())
                    .unwrap()
                    .unwrap()
                    .name()
                    .to_owned()
            })
            .collect();
        assert_eq!(names, ["parks", "roads"]);
        assert_eq!(tiles[0].dedup, None, "mixed tiles are not deduplicated");
        assert!(tiles[1].dedup.is_some());
        assert_eq!(
            (tiles[1].data.clone(), tiles[1].dedup),
            (tiles[2].data.clone(), tiles[2].dedup)
        );
        assert_eq!(encoder.stats[0].features, 3);
        assert_eq!(encoder.stats[1].zooms, Some((1, 1)));
        assert_eq!(encoder.stats[1].fields["kind"], PropKind::Str);
    }

    #[test]
    fn mvt_output_decodes() {
        let interner = [KeyInterner::new(["kind"]), KeyInterner::new(["kind"])];
        let batch = vec![tile(0, &[(1, 5, Geom::Points(&[[1, 2]]))], &interner[1])];
        let keys = interner.map(KeyInterner::freeze);
        let layers = layers();
        let settings = settings(&layers, &keys, TileFormat::Mvt);
        let tiles = TileEncoder::default()
            .encode_batch(&settings, &DedupIndex::default(), batch)
            .unwrap();
        let raw = decode_gzip(&tiles[0].data).unwrap();
        let tile = mlt_core::fast_mvt::MvtReaderRef::new(&raw).unwrap();
        assert!(format!("{tile:?}").contains("roads"));
    }

    #[test]
    fn dedup_keys_require_equal_bytes() {
        let index = DedupIndex::default();
        let key = index.key(b"fill").unwrap();
        assert_eq!(index.key(b"fill"), Some(key));
        assert_ne!(index.key(b"other"), Some(key));
    }
}
