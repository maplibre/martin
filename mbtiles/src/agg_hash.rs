use std::hash::Hasher as _;
use std::io::Write as _;

use martin_tile_utils::TileCoord;
use xxhash_rust::xxh3::Xxh3;
use xxhash_rust::xxh64::Xxh64;

use crate::{HashAlgorithm, invert_y_value};

/// The decimal digits of a `u8` and two `u32`s.
const MAX_DIGITS: usize = 3 + 10 + 10;

/// Computes `agg_tiles_hash` from the tiles as they are written, the value
/// [`calc_agg_tiles_hash`](crate::calc_agg_tiles_hash) would read back from the file.
///
/// It holds only for tiles pushed in `(zoom_level, tile_column, tile_row)` order into a file that had
/// none, each push matching a successful write. For tiles out of that order [`finish`](Self::finish)
/// gives `None`, and [`Mbtiles::finish_agg_tiles_hash`](crate::Mbtiles::finish_agg_tiles_hash) computes
/// the hash from the file instead.
pub struct AggTilesHasher {
    algorithm: HashAlgorithm,
    state: Option<State>,
    last: Option<(u8, u32, u32)>,
}

/// The running digest of each [`HashAlgorithm`].
enum State {
    Md5(md5::Context),
    Fnv(fnv::FnvHasher),
    Xxh64(Xxh64),
    Xxh3(Box<Xxh3>),
}

impl AggTilesHasher {
    /// A hasher for a file whose tile hashes use `algorithm`.
    #[must_use]
    pub fn new(algorithm: HashAlgorithm) -> Self {
        let state = match algorithm {
            HashAlgorithm::Md5 => State::Md5(md5::Context::new()),
            HashAlgorithm::Fnv1a | HashAlgorithm::Fnv1aDecimal => {
                State::Fnv(fnv::FnvHasher::default())
            }
            HashAlgorithm::Xxh64 => State::Xxh64(Xxh64::new(0)),
            HashAlgorithm::Xxh3 => State::Xxh3(Box::default()),
        };
        Self {
            algorithm,
            state: Some(state),
            last: None,
        }
    }

    /// The algorithm the hash is computed with.
    #[must_use]
    pub fn algorithm(&self) -> HashAlgorithm {
        self.algorithm
    }

    /// Adds a written tile, with `coord` in XYZ rows as [`MbtilesBulkWriter::write`](crate::MbtilesBulkWriter::write) takes it.
    pub fn push(&mut self, coord: TileCoord, data: &[u8]) {
        let key = (coord.z(), coord.x(), invert_y_value(coord.z(), coord.y()));
        if self.last.is_some_and(|last| last >= key) {
            self.state = None;
        }
        self.last = Some(key);
        let Some(state) = &mut self.state else {
            return;
        };
        let (z, x, row) = key;
        let mut digits = [0; MAX_DIGITS];
        let mut rest = &mut digits[..];
        write!(rest, "{z}{x}{row}").expect("MAX_DIGITS fits any coordinate");
        let len = MAX_DIGITS - rest.len();
        state.update(&digits[..len]);
        state.update(data);
    }

    /// Whether every tile so far came in key order, so that [`finish`](Self::finish) gives a hash.
    #[must_use]
    pub fn in_key_order(&self) -> bool {
        self.state.is_some()
    }

    /// The hash of the pushed tiles, or `None` if they came out of order.
    #[must_use]
    pub fn finish(self) -> Option<String> {
        let state = self.state?;
        if self.last.is_none() {
            return Some(self.algorithm.hash(b""));
        }
        Some(state.finish())
    }
}

impl State {
    /// Feeds `bytes` to the digest.
    fn update(&mut self, bytes: &[u8]) {
        match self {
            Self::Md5(hasher) => hasher.consume(bytes),
            Self::Fnv(hasher) => hasher.write(bytes),
            Self::Xxh64(hasher) => hasher.update(bytes),
            Self::Xxh3(hasher) => hasher.update(bytes),
        }
    }

    /// The digest as the `SQL` concat aggregates print it.
    fn finish(self) -> String {
        match self {
            Self::Md5(hasher) => format!("{:X}", hasher.finalize()),
            Self::Fnv(hasher) => format!("{:016X}", hasher.finish()),
            Self::Xxh64(hasher) => format!("{:016X}", hasher.digest()),
            Self::Xxh3(hasher) => format!("{:016X}", hasher.digest()),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        CopyDuplicateMode, HASH_ALGORITHM, MbtType, NormalizedSchema, TileDedup, anonymous_mbtiles,
        calc_agg_tiles_hash, init_mbtiles_schema,
    };

    #[rstest]
    #[case::md5(HashAlgorithm::Md5)]
    #[case::fnv1a(HashAlgorithm::Fnv1a)]
    #[case::fnv1a_decimal(HashAlgorithm::Fnv1aDecimal)]
    #[case::xxh64(HashAlgorithm::Xxh64)]
    #[case::xxh3(HashAlgorithm::Xxh3)]
    #[actix_rt::test]
    async fn tiles_in_key_order_hash_like_the_file(#[case] algorithm: HashAlgorithm) {
        let mbt_type = MbtType::Normalized {
            hash_view: false,
            schema: NormalizedSchema::DedupId,
        };
        let (mbt, mut conn) = anonymous_mbtiles("").await;
        init_mbtiles_schema(&mut conn, mbt_type, false)
            .await
            .unwrap();
        mbt.set_metadata_value(&mut conn, HASH_ALGORITHM, algorithm.as_str())
            .await
            .unwrap();
        let tiles = [
            ((0, 0, 0), b"root".as_slice(), TileDedup::Unique),
            ((1, 0, 1), b"fill", TileDedup::Key(1)),
            ((1, 0, 0), b"fill", TileDedup::Key(1)),
            ((1, 1, 1), b"leaf", TileDedup::Unique),
            ((1, 1, 0), b"fill", TileDedup::Key(1)),
            ((12, 1000, 4095), b"deep", TileDedup::Unique),
        ];
        let mut hasher = AggTilesHasher::new(algorithm);
        mbt.bulk_write(&mut conn, mbt_type, CopyDuplicateMode::Abort, |writer| {
            for ((z, x, y), data, dedup) in tiles {
                let coord = TileCoord::new_unchecked(z, x, y);
                writer.write(coord, data, dedup)?;
                hasher.push(coord, data);
            }
            Ok::<_, crate::MbtError>(())
        })
        .await
        .unwrap();

        assert_eq!(
            hasher.finish(),
            Some(calc_agg_tiles_hash(&mut conn, algorithm).await.unwrap())
        );
    }

    #[rstest]
    #[case::md5(HashAlgorithm::Md5)]
    #[case::fnv1a(HashAlgorithm::Fnv1a)]
    #[case::xxh64(HashAlgorithm::Xxh64)]
    #[case::xxh3(HashAlgorithm::Xxh3)]
    #[actix_rt::test]
    async fn no_tiles_hash_like_an_empty_file(#[case] algorithm: HashAlgorithm) {
        let (_, mut conn) = anonymous_mbtiles("").await;
        init_mbtiles_schema(&mut conn, MbtType::Flat, false)
            .await
            .unwrap();

        assert_eq!(
            AggTilesHasher::new(algorithm).finish(),
            Some(calc_agg_tiles_hash(&mut conn, algorithm).await.unwrap())
        );
    }

    #[test]
    fn tiles_out_of_key_order_give_no_hash() {
        let mut hasher = AggTilesHasher::new(HashAlgorithm::Md5);
        hasher.push(TileCoord::new_unchecked(1, 0, 0), b"a");
        hasher.push(TileCoord::new_unchecked(1, 0, 1), b"b");

        assert!(!hasher.in_key_order());
        assert_eq!(hasher.finish(), None);
    }

    #[test]
    fn a_repeated_tile_gives_no_hash() {
        let mut hasher = AggTilesHasher::new(HashAlgorithm::Md5);
        hasher.push(TileCoord::new_unchecked(1, 0, 0), b"a");
        hasher.push(TileCoord::new_unchecked(1, 0, 0), b"a");

        assert_eq!(hasher.finish(), None);
    }

    #[actix_rt::test]
    async fn finish_computes_the_hash_from_the_file_for_tiles_out_of_order() {
        let (mbt, mut conn) = anonymous_mbtiles("").await;
        init_mbtiles_schema(&mut conn, MbtType::Flat, false)
            .await
            .unwrap();
        let tiles = [((1, 0, 0), b"a".as_slice()), ((1, 0, 1), b"b")];
        let mut hasher = AggTilesHasher::new(HashAlgorithm::Md5);
        mbt.bulk_write(
            &mut conn,
            MbtType::Flat,
            CopyDuplicateMode::Abort,
            |writer| {
                for ((z, x, y), data) in tiles {
                    let coord = TileCoord::new_unchecked(z, x, y);
                    writer.write(coord, data, TileDedup::Unknown)?;
                    hasher.push(coord, data);
                }
                Ok::<_, crate::MbtError>(())
            },
        )
        .await
        .unwrap();

        let stored = mbt.finish_agg_tiles_hash(&mut conn, hasher).await.unwrap();
        assert_eq!(
            stored,
            calc_agg_tiles_hash(&mut conn, HashAlgorithm::Md5)
                .await
                .unwrap()
        );
        assert_eq!(
            mbt.get_agg_tiles_hash(&mut conn).await.unwrap(),
            Some(stored)
        );
    }
}
