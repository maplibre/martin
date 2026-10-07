//! What a data source hands the generator: layers, and partitions of features streamed in batches.

use std::ops::RangeInclusive;

use geo_types::{Coord, LineString};

use crate::props::{KeyId, KeyInterner, PropRef};
use crate::{FeatureOrder, LayerGrid, TileGenResult};

/// A layer as the source declares it; its position in [`FeatureSource::layers`] is its layer byte,
/// which is also its order inside each tile.
#[derive(Clone, Debug)]
pub struct LayerSpec {
    pub name: String,
    pub zooms: RangeInclusive<u8>,
    pub grid: LayerGrid,
    pub clip: bool,
    pub order: FeatureOrder,
    /// Property keys known up front, e.g. table columns, in their column order.
    pub known_keys: Vec<String>,
}

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
pub enum Prop {
    Bool(bool),
    I64(i64),
    F32(f32),
    F64(f64),
    Str(String),
}

impl Prop {
    #[must_use]
    pub fn as_ref(&self) -> PropRef<'_> {
        match self {
            Self::Bool(v) => PropRef::Bool(*v),
            Self::I64(v) => PropRef::I64(*v),
            Self::F32(v) => PropRef::F32(*v),
            Self::F64(v) => PropRef::F64(*v),
            Self::Str(v) => PropRef::Str(v),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceFeature {
    pub id: Option<u64>,
    pub geometry: Geometry,
    pub props: Vec<(KeyId, Prop)>,
}

/// Consecutive features of one partition and layer: feature `i` is row `first_row + i`.
#[derive(Clone, Debug)]
pub struct FeatureBatch {
    pub layer: u8,
    pub partition: u32,
    pub first_row: u64,
    pub crs: Crs,
    pub features: Vec<SourceFeature>,
}

/// A source split into partitions that can be read in parallel. Reading partitions in index order,
/// each in its own row order, must reproduce the source's order: that is the draw order the output
/// keeps, deterministically, whatever the thread timing.
pub trait FeatureSource: Sync {
    fn layers(&self) -> &[LayerSpec];

    fn partitions(&self) -> u32;

    /// Streams one partition. `keys` is parallel to [`layers`](Self::layers); keys not declared up
    /// front are interned through it. Async sources block on their own runtime here.
    fn read(
        &self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> TileGenResult<()>,
    ) -> TileGenResult<()>;
}

/// A source over prepared batches, for tests and small datasets; each batch is its own partition.
pub struct MemorySource {
    pub layers: Vec<LayerSpec>,
    pub batches: Vec<FeatureBatch>,
}

impl FeatureSource for MemorySource {
    fn layers(&self) -> &[LayerSpec] {
        &self.layers
    }

    fn partitions(&self) -> u32 {
        u32::try_from(self.batches.len()).expect("fewer than 2^32 batches")
    }

    fn read(
        &self,
        partition: u32,
        _keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> TileGenResult<()>,
    ) -> TileGenResult<()> {
        let batch = self.batches[partition as usize].clone();
        emit(FeatureBatch { partition, ..batch })
    }
}
