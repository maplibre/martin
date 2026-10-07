//! What a data source hands the generator: partitions of features streamed in batches.

use geo_types::{Coord, LineString};

use crate::props::{KeyId, KeyInterner, Prop};
use crate::{TileGenError, TileGenResult};

/// Coordinate system of a batch's geometry; projection to Web Mercator happens on render workers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crs {
    Wgs84,
    WebMercator,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    Points(Vec<Coord<f64>>),
    Lines(Vec<LineString<f64>>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceFeature {
    pub id: Option<u64>,
    pub geometry: Geometry,
    pub props: Vec<(KeyId, Prop)>,
}

/// Consecutive features of one partition and table: feature `i` is row `first_row + i`.
#[derive(Clone, Debug)]
pub struct FeatureBatch {
    /// Position of the table in the [`Plan`](crate::plan::Plan).
    pub table: u16,
    pub partition: u32,
    pub first_row: u64,
    pub crs: Crs,
    pub features: Vec<SourceFeature>,
}

/// A source split into partitions that can be read in parallel. Reading partitions in index order,
/// each in its own row order, must reproduce the source's order: that is the draw order the output
/// keeps, deterministically, whatever the thread timing.
pub trait FeatureSource: Sync {
    fn partitions(&self) -> u32;

    /// Streams one partition. `keys` is parallel to the plan's tables; keys not declared up front are
    /// interned through it. Async sources block on their own runtime here.
    fn read(
        &self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> TileGenResult<()>,
    ) -> TileGenResult<()>;

    /// One reader per render thread. Sources that must read every partition from one consistent state
    /// open their per-thread state here, all at once, so that state can be pinned before any partition is read.
    fn open_readers(&self, count: usize) -> TileGenResult<Vec<Box<dyn FeatureReader + '_>>>
    where
        Self: Sized,
    {
        Ok((0..count)
            .map(|_| Box::new(Stateless(self)) as Box<dyn FeatureReader + '_>)
            .collect())
    }
}

/// What one reader thread reads partitions through; see [`FeatureSource::open_readers`].
pub trait FeatureReader: Send {
    /// Same contract as [`FeatureSource::read`].
    fn read(
        &mut self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> TileGenResult<()>,
    ) -> TileGenResult<()>;
}

struct Stateless<'a, S>(&'a S);

impl<S: FeatureSource> FeatureReader for Stateless<'_, S> {
    fn read(
        &mut self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> TileGenResult<()>,
    ) -> TileGenResult<()> {
        self.0.read(partition, keys, emit)
    }
}

/// A source over prepared batches, for tests and small datasets; each batch is its own partition.
pub struct MemorySource {
    pub batches: Vec<FeatureBatch>,
}

impl FeatureSource for MemorySource {
    fn partitions(&self) -> u32 {
        u32::try_from(self.batches.len()).expect("fewer than 2^32 batches")
    }

    fn read(
        &self,
        partition: u32,
        _keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> TileGenResult<()>,
    ) -> TileGenResult<()> {
        let batch = self
            .batches
            .get(partition as usize)
            .ok_or(TileGenError::UnknownPartition(partition))?;
        emit(FeatureBatch {
            partition,
            ..batch.clone()
        })
    }
}
