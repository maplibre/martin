//! Encodes tiles: assembles each layer, writes MLT or MVT, compresses, and tags likely duplicates.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::sync::PoisonError;

use hotpath::wrap::std::sync::Mutex as ProfiledMutex;
use martin_tile_utils::{Encoding, Format, TileCoord};
use mlt_core::PropKind;
use mlt_core::encoder::EncoderConfig;
use mlt_core::fast_mvt::MvtTileBuilder;
use xxhash_rust::xxh3::xxh3_64;

use crate::phase2::assemble::{AssembledLayer, LayerAssembler};
use crate::props::KeyNames;
use crate::sort::{LayerRecordRef, Seq, TileRecords};
use crate::{EncodedTile, LayerGrid, TileGenResult, TileId, TileOrder};

#[derive(Clone, Copy, Debug)]
pub enum TileFormat {
    Mlt,
    Mvt,
}

impl From<TileFormat> for Format {
    fn from(format: TileFormat) -> Self {
        match format {
            TileFormat::Mlt => Self::Mlt,
            TileFormat::Mvt => Self::Mvt,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LayerInfo {
    pub name: String,
    pub grid: LayerGrid,
    pub encoder: EncoderConfig,
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
pub struct DedupIndex {
    tiles: ProfiledMutex<HashMap<u64, Vec<u8>>>,
}

#[allow(clippy::derivable_impls, reason = "the instrumented mutex has no Default")]
impl Default for DedupIndex {
    fn default() -> Self {
        Self {
            tiles: hotpath::mutex!(std::sync::Mutex::default(), label = { "dedup_index" }),
        }
    }
}

impl DedupIndex {
    fn key(&self, data: &[u8]) -> Option<u64> {
        let hash = xxh3_64(data);
        match self.tiles.lock().unwrap_or_else(PoisonError::into_inner).entry(hash) {
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
    fn widen_zooms(&mut self, min: u8, max: u8) {
        let (lo, hi) = self.zooms.unwrap_or((min, max));
        self.zooms = Some((lo.min(min), hi.max(max)));
    }

    fn add_features(&mut self, zoom: u8, features: u64) {
        self.widen_zooms(zoom, zoom);
        self.features += features;
    }

    fn add_field(&mut self, name: &str, kind: PropKind) {
        self.fields
            .entry(name.to_owned())
            .and_modify(|known| *known = known.merge(kind))
            .or_insert(kind);
    }

    fn record(&mut self, zoom: u8, layer: &AssembledLayer<'_>, names: &KeyNames) -> u64 {
        let count = layer.layer.feature_count() as u64;
        self.add_features(zoom, count);
        for &(key, kind) in layer.columns {
            self.add_field(names.name(key), kind);
        }
        count
    }

    pub fn merge(&mut self, other: &Self) {
        if let Some((lo, hi)) = other.zooms {
            self.widen_zooms(lo, hi);
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
    #[hotpath::measure]
    pub fn encode_batch(
        &mut self,
        settings: &EncodeSettings<'_>,
        dedup: &DedupIndex,
        batch: Vec<TileRecords>,
    ) -> TileGenResult<Vec<EncodedTile>> {
        if self.stats.len() < settings.layers.len() {
            self.stats.resize_with(settings.layers.len(), LayerStats::default);
        }
        let mut out = Vec::with_capacity(batch.len());
        for records in batch {
            let coord = settings.order.tile_coord(TileId::new(records.tile_id)?)?;
            let previous = match self.previous.take() {
                Some(previous) if previous.records.same_content(&records) => {
                    self.reuse(previous, records, coord, dedup)
                }
                _ => self.encode_fresh(settings, records, coord, dedup)?,
            };
            out.push(previous.tile.clone());
            self.previous = Some(previous);
        }
        Ok(out)
    }

    fn reuse(
        &mut self,
        mut previous: Previous,
        records: TileRecords,
        coord: TileCoord,
        dedup: &DedupIndex,
    ) -> Previous {
        if previous.tile.dedup_key.is_none() {
            previous.tile.dedup_key = dedup.key(&previous.tile.data);
        }
        for &(layer, features) in &previous.features {
            self.stats[usize::from(layer)].add_features(coord.z(), features);
        }
        Previous {
            records,
            tile: EncodedTile { coord, ..previous.tile },
            features: previous.features,
        }
    }

    fn encode_fresh(
        &mut self,
        settings: &EncodeSettings<'_>,
        records: TileRecords,
        coord: TileCoord,
        dedup: &DedupIndex,
    ) -> TileGenResult<Previous> {
        let mut features = Vec::new();
        let data = self.encode_tile(settings, &records, coord.z(), &mut features)?;
        let key = if records.is_fill_only() { dedup.key(&data) } else { None };
        Ok(Previous {
            records,
            tile: EncodedTile {
                coord,
                data,
                dedup_key: key,
            },
            features,
        })
    }

    fn encode_tile(
        &mut self,
        settings: &EncodeSettings<'_>,
        tile: &TileRecords,
        zoom: u8,
        features: &mut Vec<(u8, u64)>,
    ) -> TileGenResult<Vec<u8>> {
        let mut mlt = Vec::new();
        let mut mvt = MvtTileBuilder::new();
        let mut records: Vec<(Seq, &[u8])> = Vec::new();
        let mut all = tile.records().peekable();
        while let Some(&LayerRecordRef { layer, .. }) = all.peek() {
            records.clear();
            while let Some(record) = all.next_if(|record| record.layer == layer) {
                records.push((record.seq, record.bytes));
            }
            let info = &settings.layers[usize::from(layer)];
            let names = &settings.keys[usize::from(layer)];
            let assembled = self.assembler.assemble(&info.name, info.grid, names, &records)?;
            let count = self.stats[usize::from(layer)].record(zoom, &assembled, names);
            features.push((layer, count));
            match settings.format {
                TileFormat::Mlt => mlt.extend(assembled.layer.encode(info.encoder)?),
                TileFormat::Mvt => mvt = assembled.layer.write_mvt(mvt)?,
            }
        }
        let data = match settings.format {
            TileFormat::Mlt => mlt,
            TileFormat::Mvt => mvt.encode(),
        };
        Ok(martin_tile_utils::encode(data, settings.encoding)?)
    }
}

#[cfg(test)]
mod tests {
    use martin_tile_utils::decode_gzip;
    use mlt_core::encoder::{Network, Profile};
    use mlt_core::fast_mvt::MvtReaderRef;
    use mlt_core::{Decoder, Parser, PropValueRef, TileLayer};

    use super::*;
    use crate::TileGenError;
    use crate::props::KeyInterner;
    use crate::sort::record::{EncodedProps, Geom, encode};

    fn layer(name: &str) -> LayerInfo {
        LayerInfo {
            name: name.to_owned(),
            grid: LayerGrid {
                extent: 4096,
                buffer: 64,
            },
            encoder: EncoderConfig::from_profile(Profile::Web(Network::Offline)),
        }
    }

    fn mlt_layers(data: &[u8]) -> Vec<TileLayer> {
        Parser::default()
            .parse_layers(data)
            .unwrap()
            .into_iter()
            .map(|l| l.into_tile(&mut Decoder::default()).unwrap().unwrap())
            .collect()
    }

    fn feature(row: u64, props: &[(&str, PropValueRef<'_>)], interner: &KeyInterner) -> Vec<u8> {
        let mut encoded = EncodedProps::default();
        for &(key, value) in props {
            encoded.push(interner.intern(key), value);
        }
        let mut bytes = Vec::new();
        encode(&mut bytes, Some(row), None, &encoded, Geom::Points(&[[0, 0]]));
        bytes
    }

    fn fill() -> Vec<u8> {
        let mut bytes = Vec::new();
        encode(&mut bytes, None, None, &EncodedProps::default(), Geom::Fill);
        bytes
    }

    #[test]
    fn layers_are_written_in_layer_order() {
        let interners = [KeyInterner::new::<&str>([]), KeyInterner::new::<&str>([])];
        let mut tile = TileRecords::new(1);
        tile.push_record(0, Seq::new(0, 1).unwrap(), &feature(1, &[], &interners[0]));
        tile.push_record(1, Seq::new(0, 2).unwrap(), &feature(2, &[], &interners[1]));
        let keys = interners.map(KeyInterner::freeze);
        let layers = [layer("parks"), layer("roads")];
        let settings = EncodeSettings {
            layers: &layers,
            keys: &keys,
            format: TileFormat::Mlt,
            encoding: Encoding::Gzip,
            order: TileOrder::Tms,
        };

        let tiles = TileEncoder::default()
            .encode_batch(&settings, &DedupIndex::default(), vec![tile])
            .unwrap();

        let decoded = mlt_layers(&decode_gzip(&tiles[0].data).unwrap());
        let names: Vec<_> = decoded.iter().map(TileLayer::name).collect();
        assert_eq!(names, ["parks", "roads"]);
    }

    #[test]
    fn tms_order_flips_the_row() {
        let interner = KeyInterner::new::<&str>([]);
        let mut tile = TileRecords::new(1);
        tile.push_record(0, Seq::new(0, 1).unwrap(), &feature(1, &[], &interner));
        let keys = [interner.freeze()];
        let layers = [layer("parks")];
        let settings = EncodeSettings {
            layers: &layers,
            keys: &keys,
            format: TileFormat::Mvt,
            encoding: Encoding::Uncompressed,
            order: TileOrder::Tms,
        };

        let tiles = TileEncoder::default()
            .encode_batch(&settings, &DedupIndex::default(), vec![tile])
            .unwrap();

        assert_eq!(tiles[0].coord, TileCoord::new_unchecked(1, 0, 1));
    }

    #[test]
    fn hilbert_order_starts_at_the_top_left() {
        let interner = KeyInterner::new::<&str>([]);
        let mut tile = TileRecords::new(1);
        tile.push_record(0, Seq::new(0, 1).unwrap(), &feature(1, &[], &interner));
        let keys = [interner.freeze()];
        let layers = [layer("parks")];
        let settings = EncodeSettings {
            layers: &layers,
            keys: &keys,
            format: TileFormat::Mvt,
            encoding: Encoding::Uncompressed,
            order: TileOrder::Hilbert,
        };

        let tiles = TileEncoder::default()
            .encode_batch(&settings, &DedupIndex::default(), vec![tile])
            .unwrap();

        assert_eq!(tiles[0].coord, TileCoord::new_unchecked(1, 0, 0));
    }

    #[test]
    fn invalid_tile_id_is_an_error() {
        let keys: [KeyNames; 0] = [];
        let layers: [LayerInfo; 0] = [];
        let settings = EncodeSettings {
            layers: &layers,
            keys: &keys,
            format: TileFormat::Mvt,
            encoding: Encoding::Uncompressed,
            order: TileOrder::Tms,
        };

        let err = TileEncoder::default()
            .encode_batch(&settings, &DedupIndex::default(), vec![TileRecords::new(u64::MAX)])
            .unwrap_err();

        assert!(matches!(err, TileGenError::InvalidTileId(u64::MAX)));
    }

    #[test]
    fn equal_fill_tiles_share_a_dedup_key() {
        let interner = KeyInterner::new::<&str>([]);
        let mut first = TileRecords::new(1);
        first.push_record(0, Seq::new(0, 1).unwrap(), &fill());
        let mut second = TileRecords::new(2);
        second.push_record(0, Seq::new(0, 9).unwrap(), &fill());
        let keys = [interner.freeze()];
        let layers = [layer("water")];
        let settings = EncodeSettings {
            layers: &layers,
            keys: &keys,
            format: TileFormat::Mlt,
            encoding: Encoding::Gzip,
            order: TileOrder::Tms,
        };

        let tiles = TileEncoder::default()
            .encode_batch(&settings, &DedupIndex::default(), vec![first, second])
            .unwrap();

        assert!(tiles[0].dedup_key.is_some());
        assert_eq!(tiles[0].dedup_key, tiles[1].dedup_key);
        assert_eq!(tiles[0].data, tiles[1].data);
        assert_ne!(tiles[0].coord, tiles[1].coord);
    }

    #[test]
    fn dedup_index_is_shared_between_encoders() {
        let interner = KeyInterner::new::<&str>([]);
        let mut first = TileRecords::new(1);
        first.push_record(0, Seq::new(0, 1).unwrap(), &fill());
        let mut second = TileRecords::new(2);
        second.push_record(0, Seq::new(0, 1).unwrap(), &fill());
        let keys = [interner.freeze()];
        let layers = [layer("water")];
        let settings = EncodeSettings {
            layers: &layers,
            keys: &keys,
            format: TileFormat::Mlt,
            encoding: Encoding::Gzip,
            order: TileOrder::Tms,
        };
        let dedup = DedupIndex::default();

        let from_first = TileEncoder::default()
            .encode_batch(&settings, &dedup, vec![first])
            .unwrap();
        let from_second = TileEncoder::default()
            .encode_batch(&settings, &dedup, vec![second])
            .unwrap();

        assert!(from_first[0].dedup_key.is_some());
        assert_eq!(from_first[0].dedup_key, from_second[0].dedup_key);
    }

    #[test]
    fn tiles_with_features_are_not_deduplicated() {
        let interner = KeyInterner::new::<&str>([]);
        let mut tile = TileRecords::new(1);
        tile.push_record(0, Seq::new(0, 1).unwrap(), &fill());
        tile.push_record(0, Seq::new(0, 2).unwrap(), &feature(2, &[], &interner));
        let keys = [interner.freeze()];
        let layers = [layer("water")];
        let settings = EncodeSettings {
            layers: &layers,
            keys: &keys,
            format: TileFormat::Mlt,
            encoding: Encoding::Gzip,
            order: TileOrder::Tms,
        };

        let tiles = TileEncoder::default()
            .encode_batch(&settings, &DedupIndex::default(), vec![tile])
            .unwrap();

        assert_eq!(tiles[0].dedup_key, None);
    }

    #[test]
    fn mvt_output_holds_the_layer_and_its_properties() {
        let interner = KeyInterner::new::<&str>([]);
        let mut tile = TileRecords::new(0);
        tile.push_record(
            0,
            Seq::new(0, 5).unwrap(),
            &feature(5, &[("kind", PropValueRef::Str("park"))], &interner),
        );
        let keys = [interner.freeze()];
        let layers = [layer("roads")];
        let settings = EncodeSettings {
            layers: &layers,
            keys: &keys,
            format: TileFormat::Mvt,
            encoding: Encoding::Gzip,
            order: TileOrder::Tms,
        };

        let tiles = TileEncoder::default()
            .encode_batch(&settings, &DedupIndex::default(), vec![tile])
            .unwrap();

        let raw = decode_gzip(&tiles[0].data).unwrap();
        let mvt = MvtReaderRef::new(&raw).unwrap();
        let decoded: Vec<_> = mvt.layers().collect();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].name(), "roads");
        assert_eq!(decoded[0].feature_count(), 1);
        assert_eq!(decoded[0].keys(), ["kind"]);
    }

    #[test]
    fn columns_follow_known_keys_then_name_order() {
        let interner = KeyInterner::new(["zeta", "alpha"]);
        let mut tile = TileRecords::new(0);
        tile.push_record(
            0,
            Seq::new(0, 1).unwrap(),
            &feature(
                1,
                &[
                    ("b", PropValueRef::I64(1)),
                    ("alpha", PropValueRef::I64(1)),
                    ("a", PropValueRef::I64(1)),
                    ("zeta", PropValueRef::I64(1)),
                ],
                &interner,
            ),
        );
        let keys = [interner.freeze()];
        let layers = [layer("parks")];
        let settings = EncodeSettings {
            layers: &layers,
            keys: &keys,
            format: TileFormat::Mlt,
            encoding: Encoding::Uncompressed,
            order: TileOrder::Tms,
        };

        let tiles = TileEncoder::default()
            .encode_batch(&settings, &DedupIndex::default(), vec![tile])
            .unwrap();

        let decoded = mlt_layers(&tiles[0].data);
        assert_eq!(decoded[0].property_names(), ["zeta", "alpha", "a", "b"]);
    }

    #[test]
    fn column_kinds_are_merged_across_features() {
        let interner = KeyInterner::new(["signed", "float", "mixed", "unsigned", "widened"]);
        let mut tile = TileRecords::new(0);
        let rows: [&[(&str, PropValueRef<'_>)]; 2] = [
            &[
                ("signed", PropValueRef::I64(-1)),
                ("float", PropValueRef::F32(1.5)),
                ("mixed", PropValueRef::I64(1)),
                ("unsigned", PropValueRef::I64(7)),
                ("widened", PropValueRef::I64(3)),
            ],
            &[
                ("signed", PropValueRef::I64(5)),
                ("float", PropValueRef::F64(2.5)),
                ("mixed", PropValueRef::Bool(true)),
                ("widened", PropValueRef::F64(1.5)),
            ],
        ];
        for (row, props) in (1..).zip(rows) {
            tile.push_record(0, Seq::new(0, row).unwrap(), &feature(row, props, &interner));
        }
        let keys = [interner.freeze()];
        let layers = [layer("parks")];
        let settings = EncodeSettings {
            layers: &layers,
            keys: &keys,
            format: TileFormat::Mlt,
            encoding: Encoding::Uncompressed,
            order: TileOrder::Tms,
        };

        let mut encoder = TileEncoder::default();

        encoder
            .encode_batch(&settings, &DedupIndex::default(), vec![tile])
            .unwrap();

        assert_eq!(
            encoder.stats[0].fields,
            BTreeMap::from([
                ("signed".to_owned(), PropKind::I64),
                ("float".to_owned(), PropKind::F64),
                ("mixed".to_owned(), PropKind::Str),
                ("unsigned".to_owned(), PropKind::U64),
                ("widened".to_owned(), PropKind::F64),
            ])
        );
    }

    #[test]
    fn a_tile_does_not_inherit_columns_of_the_previous_tile() {
        let interner = KeyInterner::new::<&str>([]);
        let mut first = TileRecords::new(0);
        first.push_record(
            0,
            Seq::new(0, 1).unwrap(),
            &feature(
                1,
                &[("a", PropValueRef::I64(1)), ("b", PropValueRef::I64(1))],
                &interner,
            ),
        );
        let mut second = TileRecords::new(1);
        second.push_record(
            0,
            Seq::new(0, 2).unwrap(),
            &feature(2, &[("c", PropValueRef::I64(1))], &interner),
        );
        let mut third = TileRecords::new(2);
        third.push_record(
            0,
            Seq::new(0, 3).unwrap(),
            &feature(3, &[("b", PropValueRef::I64(1))], &interner),
        );
        let keys = [interner.freeze()];
        let layers = [layer("parks")];
        let settings = EncodeSettings {
            layers: &layers,
            keys: &keys,
            format: TileFormat::Mlt,
            encoding: Encoding::Uncompressed,
            order: TileOrder::Tms,
        };

        let tiles = TileEncoder::default()
            .encode_batch(&settings, &DedupIndex::default(), vec![first, second, third])
            .unwrap();

        let columns: Vec<_> = tiles
            .iter()
            .map(|t| mlt_layers(&t.data)[0].property_names().to_vec())
            .collect();
        assert_eq!(columns, [vec!["a", "b"], vec!["c"], vec!["b"]]);
    }

    #[test]
    fn stats_count_features_zooms_and_fields_per_layer() {
        let interners = [KeyInterner::new::<&str>([]), KeyInterner::new::<&str>([])];
        let mut z0 = TileRecords::new(0);
        z0.push_record(
            0,
            Seq::new(0, 1).unwrap(),
            &feature(1, &[("kind", PropValueRef::Str("park"))], &interners[0]),
        );
        let mut z2 = TileRecords::new(5);
        z2.push_record(
            0,
            Seq::new(0, 2).unwrap(),
            &feature(2, &[("area", PropValueRef::F64(1.5))], &interners[0]),
        );
        z2.push_record(
            0,
            Seq::new(0, 3).unwrap(),
            &feature(3, &[("area", PropValueRef::F64(2.5))], &interners[0]),
        );
        let keys = interners.map(KeyInterner::freeze);
        let layers = [layer("parks"), layer("roads")];
        let settings = EncodeSettings {
            layers: &layers,
            keys: &keys,
            format: TileFormat::Mlt,
            encoding: Encoding::Uncompressed,
            order: TileOrder::Tms,
        };
        let mut encoder = TileEncoder::default();

        encoder
            .encode_batch(&settings, &DedupIndex::default(), vec![z0, z2])
            .unwrap();

        assert_eq!(
            encoder.stats,
            [
                LayerStats {
                    zooms: Some((0, 2)),
                    features: 3,
                    fields: BTreeMap::from([("kind".to_owned(), PropKind::Str), ("area".to_owned(), PropKind::F64),]),
                },
                LayerStats::default(),
            ]
        );
    }

    #[test]
    fn reused_tiles_count_in_the_stats_of_their_own_zoom() {
        let interner = KeyInterner::new::<&str>([]);
        let mut z1 = TileRecords::new(1);
        z1.push_record(0, Seq::new(0, 1).unwrap(), &fill());
        let mut z2 = TileRecords::new(5);
        z2.push_record(0, Seq::new(0, 1).unwrap(), &fill());
        let keys = [interner.freeze()];
        let layers = [layer("water")];
        let settings = EncodeSettings {
            layers: &layers,
            keys: &keys,
            format: TileFormat::Mlt,
            encoding: Encoding::Uncompressed,
            order: TileOrder::Tms,
        };
        let mut encoder = TileEncoder::default();

        encoder
            .encode_batch(&settings, &DedupIndex::default(), vec![z1, z2])
            .unwrap();

        assert_eq!(encoder.stats[0].zooms, Some((1, 2)));
        assert_eq!(encoder.stats[0].features, 2);
    }

    #[test]
    fn stats_merge_widens_zooms_and_adds_features_and_fields() {
        let mut left = LayerStats {
            zooms: Some((3, 5)),
            features: 2,
            fields: BTreeMap::from([("n".to_owned(), PropKind::I64)]),
        };
        let right = LayerStats {
            zooms: Some((1, 4)),
            features: 5,
            fields: BTreeMap::from([("n".to_owned(), PropKind::F64), ("s".to_owned(), PropKind::Str)]),
        };

        left.merge(&right);

        assert_eq!(
            left,
            LayerStats {
                zooms: Some((1, 5)),
                features: 7,
                fields: BTreeMap::from([("n".to_owned(), PropKind::F64), ("s".to_owned(), PropKind::Str),]),
            }
        );
    }

    #[test]
    fn stats_merge_of_an_empty_side_keeps_the_other() {
        let mut empty = LayerStats::default();
        let other = LayerStats {
            zooms: Some((2, 2)),
            features: 1,
            fields: BTreeMap::new(),
        };

        empty.merge(&other);

        assert_eq!(empty, other);
    }

    #[test]
    fn dedup_keys_require_equal_bytes() {
        let index = DedupIndex::default();
        let key = index.key(b"fill").unwrap();
        assert_eq!(index.key(b"fill"), Some(key));
        assert_ne!(index.key(b"other"), Some(key));
    }
}
