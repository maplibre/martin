//! An OpenStreetMap PBF file as a [`FeatureSource`]: tagged nodes as points, tagged ways as lines, closed ways also
//! as polygons, and `multipolygon` and `boundary` relations as polygons.
//!
//! A closed way is read as Planetiler reads it: as a polygon and as its [`Geometry::Outline`], both with the way's
//! feature id, unless `area=yes` makes it only a polygon or `area=no` only a line. Its tags do not decide it.
//!
//! Each table of the plan reads the file in byte ranges of whole blocks, one partition each. A table's row filter is
//! pushed down three ways: object types no layer takes are not decoded, blocks whose strings cannot hold a match are
//! skipped by osmflow before they are decoded, and each object is checked before its way nodes are looked up.

mod condition;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use bytes::BytesMut;
use geo::{BoundingRect as _, Intersects as _};
use geo_types::{Coord, Polygon, Rect};
use osmflow::nodes::{FileStore as _, MemoryReader, MemoryStore, NodeRead as _, NodeReader, PagedReader, PagedStore};
use osmflow::{BlobRange, Block, GeometryError, Location, Locator, MemberType, PbfError, PbfReader, Select, TagKey};
use tilejson::Bounds;
use tracing::{debug, info};

use crate::osm::condition::{Row, matches, may_be_in_block, may_have_type};
use crate::plan::{ColumnDef, Condition, GeometryType, Plan, RowFilter, ValueKind};
use crate::props::{KeyId, KeyInterner, Prop, SourceStr};
use crate::source::{
    Crs, FeatureBatch, FeatureReader, FeatureSource, Geometry, SourceError, SourceFeature, SourceResult,
};
use crate::{MAX_ZOOM, TileGenError, TileGenResult};

/// The column holding what an object is: `node`, `way` or `relation`.
pub const OSM_TYPE: &str = "osm_type";

/// Features per batch sent to the render workers.
const BATCH_FEATURES: usize = 4096;
/// Bytes of a batch's shared string buffer, allocated at once.
const ARENA_BYTES: usize = 64 << 10;

/// The columns of an OSM table reading `tags`: [`OSM_TYPE`], then each tag as a string column.
#[must_use]
pub fn columns(tags: &[String]) -> Vec<ColumnDef> {
    std::iter::once(OSM_TYPE)
        .chain(tags.iter().map(String::as_str))
        .map(|name| ColumnDef::Value(name.to_owned(), Some(ValueKind::String)))
        .collect()
}

/// Where the node locations that ways are built from are kept while the file is read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeStore {
    /// A hash map in memory, about 30 bytes per node: for extracts up to a country.
    Memory,
    /// A file in this directory, about 5 bytes per node, deleted once the run ends.
    Paged(PathBuf),
    /// Locations shared by every read of the file, loaded by the first read that needs them.
    Shared(SharedNodes),
}

/// The node locations of one file, loaded once for several reads of it by the first that needs them.
#[derive(Clone)]
pub struct SharedNodes {
    /// The directory of a paged store, or `None` for one in memory.
    paged: Option<PathBuf>,
    /// The locations, once a read loaded them.
    store: Arc<Mutex<Option<Arc<Store>>>>,
}

impl SharedNodes {
    /// The locations of every node of the file at `path`, loaded from `file` unless an earlier read did.
    fn get(&self, file: &PbfReader, path: &Path) -> Result<Arc<Store>, OsmError> {
        let mut store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(store) = &*store {
            return Ok(Arc::clone(store));
        }
        let loaded = Arc::new(load_nodes(file, path, self.paged.as_deref())?);
        *store = Some(Arc::clone(&loaded));
        Ok(loaded)
    }
}

impl fmt::Debug for SharedNodes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedNodes")
            .field("paged", &self.paged)
            .finish_non_exhaustive()
    }
}

/// Two shared stores are equal when they are the same store.
impl PartialEq for SharedNodes {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.store, &other.store)
    }
}

impl Eq for SharedNodes {}

impl NodeStore {
    /// Files up to this size keep their nodes in memory: about 3 GB at the peak of loading.
    pub const MEMORY_UP_TO: u64 = 512 << 20;

    /// The store for the file at `path`: [`NodeStore::Memory`] up to [`Self::MEMORY_UP_TO`] bytes, else a file in
    /// `temp_dir`.
    ///
    /// # Errors
    /// If the file cannot be read.
    pub fn for_file(path: &Path, temp_dir: &Path) -> std::io::Result<Self> {
        Ok(if std::fs::metadata(path)?.len() <= Self::MEMORY_UP_TO {
            Self::Memory
        } else {
            Self::Paged(temp_dir.to_path_buf())
        })
    }

    /// This store, shared by every read of its file and loaded by the first read that needs node locations.
    #[must_use]
    pub fn shared(self) -> Self {
        let paged = match self {
            Self::Shared(_) => return self,
            Self::Memory => None,
            Self::Paged(dir) => Some(dir),
        };
        Self::Shared(SharedNodes {
            paged,
            store: Arc::default(),
        })
    }
}

/// Which objects of a file a table reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Objects {
    /// Those some layer may take, as bulk mode reads them.
    #[default]
    Taken,
    /// Every tagged object, also those no layer takes.
    Every,
    /// Only the object with this feature id, whether a layer takes it or not.
    One(u64),
    /// Those for which this condition holds, whether a layer takes them or not.
    Matching(Condition),
}

/// One table of the plan, read from an OSM PBF file.
#[derive(Clone, Debug)]
pub struct OsmTable {
    /// The `.osm.pbf` file.
    pub path: PathBuf,
    /// Where the node locations of its ways and relations are kept.
    pub node_store: NodeStore,
    /// Only objects touching these WGS84 bounds, if set.
    pub bounds: Option<Bounds>,
    /// Which objects are read.
    pub objects: Objects,
}

/// Why reading an OSM file failed.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum OsmError {
    /// The file cannot be read, or is not a valid PBF file.
    #[error("Cannot read {}: {error}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        error: PbfError,
    },

    /// The node store failed.
    #[error("Cannot keep the node locations of {}: {error}", path.display())]
    NodeStore {
        path: PathBuf,
        #[source]
        error: osmflow::nodes::NodeStoreError,
    },

    /// The temporary directory of a node store cannot be created.
    #[error("Cannot create a node store in {}: {error}", dir.display())]
    TempFile {
        dir: PathBuf,
        #[source]
        error: std::io::Error,
    },

    /// A feature id that no OSM object has, as it does not end in 1, 2 or 3.
    #[error(
        "{id} is not the feature id of an OSM object, which ends in 1 for a node, 2 for a way and 3 for a relation"
    )]
    FeatureId { id: u64 },

    /// Blocks continue their string table, so the block filters may have skipped objects.
    #[error("Cannot read {}: {} blocks continue their string table after their first group; please report it at https://github.com/nyurik/osmflow/issues", path.display(), blocks)]
    SplitStringTables { path: PathBuf, blocks: u64 },
}

impl From<OsmError> for SourceError {
    fn from(err: OsmError) -> Self {
        Self::Other(Box::new(err))
    }
}

impl From<OsmError> for TileGenError {
    fn from(err: OsmError) -> Self {
        SourceError::from(err).into()
    }
}

/// OSM PBF files, one per table of the plan, each split into partitions of whole blocks, then of relations.
pub struct OsmSource {
    plan: Plan,
    scans: Vec<Scan>,
    /// Each partition's scan and what it reads, in order.
    partitions: Vec<(u16, Partition)>,
}

/// What one partition reads.
#[derive(Clone, Debug)]
enum Partition {
    /// The nodes and ways of these blocks.
    Blocks(BlobRange),
    /// These relations of the scan, by position.
    Relations(std::ops::Range<usize>),
}

/// Relations per partition: a relation is assembled from its ways, which takes about as long as reading a block.
const RELATIONS_PER_PARTITION: usize = 256;

/// A relation some layer may take, with what its area is made of.
struct Relation {
    id: i64,
    /// The value of each scanned column, by position.
    values: Vec<Option<String>>,
    /// Its member ways, by id.
    ways: Vec<i64>,
}

/// One table's file, and what to read from it.
struct Scan {
    path: PathBuf,
    file: PbfReader,
    select: Select,
    /// The rows some layer takes, if they can be told apart.
    condition: Option<Condition>,
    /// The only object read, if one.
    only: Option<(Kind, i64)>,
    /// The scanned columns: [`OSM_TYPE`] or a tag key, by position.
    columns: Vec<String>,
    /// Only objects touching this box, if set.
    bounds: Option<Rect>,
    store: Option<Arc<Store>>,
    /// The closed ways also read as outlines, by their tags as lines, if not every one: none without a layer of
    /// lines, else those one takes.
    outlines: Option<Condition>,
    /// The `multipolygon` and `boundary` relations some layer may take, by id.
    relations: Vec<Relation>,
    /// The node ids of the relations' member ways, by way id.
    member_ways: HashMap<i64, Vec<i64>>,
}

/// The node locations of a file whose ways are read.
enum Store {
    /// A hash map in memory.
    Memory(MemoryStore),
    /// The store, and the temporary directory its file is in, deleted after the store is closed.
    Paged {
        store: Box<PagedStore>,
        _dir: tempfile::TempDir,
    },
}

impl OsmSource {
    /// Plans `tables` and opens each table's file, splitting it into partitions of about `partition_bytes`; loads
    /// the node locations of each file whose ways some layer takes.
    ///
    /// # Errors
    /// On an invalid plan, or if a file or its node store cannot be read.
    pub fn new(tables: Vec<(crate::plan::TableDef, OsmTable)>, partition_bytes: u64) -> TileGenResult<Self> {
        let (defs, osm): (Vec<_>, Vec<_>) = tables.into_iter().unzip();
        let types: Vec<Vec<GeometryType>> = defs.iter().map(layer_types).collect();
        let plan = Plan::new(defs)?;
        let mut scans = Vec::with_capacity(osm.len());
        let mut partitions = Vec::new();
        for ((index, table), types) in osm.into_iter().enumerate().zip(types) {
            let scan = Scan::open(&plan, index, table, &types)?;
            let index = u16::try_from(index).expect("the plan has fewer than 2^16 tables");
            let ranges = scan.file.split(partition_bytes).map_err(read_error(&scan.path))?;
            partitions.extend(ranges.into_iter().map(|range| (index, Partition::Blocks(range))));
            for start in (0..scan.relations.len()).step_by(RELATIONS_PER_PARTITION) {
                let end = (start + RELATIONS_PER_PARTITION).min(scan.relations.len());
                partitions.push((index, Partition::Relations(start..end)));
            }
            scans.push(scan);
        }
        Ok(Self {
            plan,
            scans,
            partitions,
        })
    }

    /// The plan the source reads its tables for.
    #[must_use]
    pub fn plan(&self) -> &Plan {
        &self.plan
    }
}

impl Scan {
    /// Opens table `index` of `plan`: its pushdown, its relations, and its node store if it reads ways or relations.
    fn open(plan: &Plan, index: usize, table: OsmTable, layer_types: &[GeometryType]) -> Result<Self, OsmError> {
        let read_error = read_error(&table.path);
        let condition = match (&table.objects, plan.row_filter(index, 0..=MAX_ZOOM)) {
            (Objects::Taken, RowFilter::Matching(condition)) => Some(condition),
            (Objects::Matching(condition), _) => Some(condition.clone()),
            (Objects::Taken, RowFilter::All | RowFilter::Untranslatable(_)) | (Objects::Every | Objects::One(_), _) => {
                None
            }
        };
        let outlines = match (
            &table.objects,
            plan.typed_row_filter(index, 0..=MAX_ZOOM, GeometryType::Line),
        ) {
            (_, RowFilter::Matching(Condition::Const(false))) => Some(Condition::Const(false)),
            (Objects::Taken, RowFilter::Matching(condition)) => Some(condition),
            (Objects::Matching(condition), _) => Some(condition.clone()),
            (Objects::Taken, RowFilter::All | RowFilter::Untranslatable(_)) | (Objects::Every | Objects::One(_), _) => {
                None
            }
        };
        let only = match table.objects {
            Objects::One(id) => Some(Kind::of_feature(id).ok_or(OsmError::FeatureId { id })?),
            Objects::Taken | Objects::Every | Objects::Matching(_) => None,
        };
        let columns = plan.columns(index).to_vec();
        let may = |kind: Kind, types: &[GeometryType]| {
            let taken = table.objects != Objects::Taken || types.iter().any(|t| layer_types.contains(t));
            taken && only.is_none_or(|(k, _)| k == kind) && condition.as_ref().is_none_or(|c| may_have_type(c, types))
        };
        let points = may(Kind::Node, &[GeometryType::Point]);
        let ways = may(Kind::Way, &[GeometryType::Line, GeometryType::Polygon]);
        let areas = may(Kind::Relation, &[GeometryType::Polygon]);
        let mut select = Select::all().without_relations().without_metadata();
        if !points {
            select = select.without_nodes();
        }
        if !ways {
            select = select.without_ways();
        }
        let base = PbfReader::open(&table.path).map_err(read_error)?;
        let tags: Vec<String> = columns.iter().filter(|c| *c != OSM_TYPE).cloned().collect();
        let block_test = |types: &[GeometryType]| {
            let (condition, tags, types) = (condition.clone(), tags.clone(), types.to_vec());
            move |strings: &osmflow::BlockStrings<'_>| {
                condition.as_ref().is_none_or(|condition| {
                    may_be_in_block(condition, &types, &|name| tags.iter().any(|t| t == name), strings)
                })
            }
        };
        let file = base
            .clone()
            .nodes_where(block_test(&[GeometryType::Point]))
            .ways_where(block_test(&[GeometryType::Line, GeometryType::Polygon]));
        let (relations, member_ways) = if areas {
            let test = block_test(&[GeometryType::Polygon]);
            let relations = base
                .clone()
                .relations_where(move |s| (s.has("multipolygon") || s.has("boundary")) && test(s));
            let only = only.map(|(_, id)| id);
            let relations = read_relations(&relations, condition.as_ref(), only, &columns).map_err(read_error)?;
            let ways = member_ways(&base, &relations).map_err(read_error)?;
            (relations, ways)
        } else {
            (Vec::new(), HashMap::new())
        };
        let store = match &table.node_store {
            _ if !ways && relations.is_empty() => None,
            NodeStore::Shared(nodes) => Some(nodes.get(&base, &table.path)?),
            NodeStore::Memory => Some(Arc::new(load_nodes(&base, &table.path, None)?)),
            NodeStore::Paged(dir) => Some(Arc::new(load_nodes(&base, &table.path, Some(dir))?)),
        };
        info!(
            osm.file = %table.path.display(),
            osm.nodes = points,
            osm.ways = ways,
            osm.relations = relations.len(),
            osm.pushdown = condition.is_some(),
            "Reading OSM file"
        );
        Ok(Self {
            path: table.path,
            file,
            select,
            condition,
            only,
            columns,
            bounds: table.bounds.map(|b| Rect::new((b.left, b.bottom), (b.right, b.top))),
            store,
            outlines,
            relations,
            member_ways,
        })
    }

    /// Whether the closed way whose scanned columns `tag` gives by position is read as an outline too.
    fn takes_outline<'t>(&self, tag: &dyn Fn(usize) -> Option<&'t str>) -> bool {
        self.outlines.as_ref().is_none_or(|condition| {
            let row = ObjectRow {
                columns: &self.columns,
                kind: Kind::Way,
                tag,
                geometry: GeometryType::Line,
            };
            matches(condition, &row)
        })
    }

    /// Whether the object of type `kind` with OSM id `id` is read, when the table reads only one.
    fn wants(&self, kind: Kind, id: i64) -> bool {
        self.only.is_none_or(|only| only == (kind, id))
    }

    /// Whether `condition` takes the object of type `kind` whose scanned columns `tag` gives by position, with
    /// a geometry of type `geometry`; every object if the scan has no condition.
    fn takes<'t>(&self, kind: Kind, geometry: GeometryType, tag: &dyn Fn(usize) -> Option<&'t str>) -> bool {
        self.condition.as_ref().is_none_or(|condition| {
            let row = ObjectRow {
                columns: &self.columns,
                kind,
                tag,
                geometry,
            };
            matches(condition, &row)
        })
    }
}

/// A function turning a [`PbfError`] into an [`OsmError`] naming the file at `path`.
fn read_error(path: &Path) -> impl Fn(PbfError) -> OsmError + Copy + '_ {
    |error| OsmError::Read {
        path: path.to_path_buf(),
        error,
    }
}

/// The key of each of `columns` in `block`, by position; `None` if no object of the block has that tag.
fn column_keys<'b>(block: &'b Block, columns: &[String]) -> Vec<Option<TagKey<'b>>> {
    columns.iter().map(|c| block.tag_key(c)).collect()
}

/// The geometry types some layer of `table` takes, whatever its `where`.
fn layer_types(table: &crate::plan::TableDef) -> Vec<GeometryType> {
    let all = vec![GeometryType::Point, GeometryType::Line, GeometryType::Polygon];
    let mut types = Vec::new();
    for layer in &table.layers {
        if layer.geometry.is_empty() {
            return all;
        }
        for entry in &layer.geometry {
            let Some(kind) = entry.takes else {
                return all;
            };
            if !types.contains(&kind) {
                types.push(kind);
            }
        }
    }
    types
}

/// The `multipolygon` and `boundary` relations of `file` that `condition` may take as polygons, by id; only relation
/// `only` if set.
fn read_relations(
    file: &PbfReader,
    condition: Option<&Condition>,
    only: Option<i64>,
    columns: &[String],
) -> Result<Vec<Relation>, PbfError> {
    let found = Mutex::new(Vec::new());
    let select = Select::all().without_nodes().without_ways().without_metadata();
    file.par_unordered(select, |block| {
        let keys = column_keys(&block, columns);
        let mut kept = Vec::new();
        for relation in block.relations() {
            if only.is_some_and(|id| id != relation.id()) {
                continue;
            }
            let area = relation
                .tags()
                .any(|(key, value)| key == "type" && matches!(value, "multipolygon" | "boundary"));
            if !area {
                continue;
            }
            let tag = |pos: usize| relation.tag(keys[pos]?).filter(|v| !v.is_empty());
            if let Some(condition) = condition {
                let row = ObjectRow {
                    columns,
                    kind: Kind::Relation,
                    tag: &tag,
                    geometry: GeometryType::Polygon,
                };
                if !matches(condition, &row) {
                    continue;
                }
            }
            kept.push(Relation {
                id: relation.id(),
                values: (0..columns.len()).map(|pos| tag(pos).map(str::to_owned)).collect(),
                ways: relation
                    .members()
                    .filter(|member| member.kind == MemberType::Way)
                    .map(|member| member.id)
                    .collect(),
            });
        }
        found.lock().unwrap_or_else(PoisonError::into_inner).extend(kept);
        Ok(())
    })?;
    let mut relations = found.into_inner().unwrap_or_else(PoisonError::into_inner);
    relations.sort_unstable_by_key(|relation| relation.id);
    Ok(relations)
}

/// The node ids of every way of `file` that is a member of one of `relations`, by way id.
fn member_ways(file: &PbfReader, relations: &[Relation]) -> Result<HashMap<i64, Vec<i64>>, PbfError> {
    let wanted: HashSet<i64> = relations.iter().flat_map(|r| r.ways.iter().copied()).collect();
    if wanted.is_empty() {
        return Ok(HashMap::new());
    }
    let found = Mutex::new(HashMap::with_capacity(wanted.len()));
    let select = Select::all()
        .without_nodes()
        .without_relations()
        .without_metadata()
        .without_tags();
    file.par_unordered(select, |block| {
        let kept: Vec<(i64, Vec<i64>)> = block
            .ways()
            .filter(|way| wanted.contains(&way.id()))
            .map(|way| (way.id(), way.refs().collect()))
            .collect();
        found.lock().unwrap_or_else(PoisonError::into_inner).extend(kept);
        Ok(())
    })?;
    Ok(found.into_inner().unwrap_or_else(PoisonError::into_inner))
}

/// Loads the locations of every node of `file`, at `path`, into memory, or into a file in `paged`.
fn load_nodes(file: &PbfReader, path: &Path, paged: Option<&Path>) -> Result<Store, OsmError> {
    let read_error = read_error(path);
    let started = std::time::Instant::now();
    let (store, count) = match paged {
        None => {
            let mut store = MemoryStore::new();
            let count = file.load_nodes(&mut store).map_err(read_error)?;
            (Store::Memory(store), count)
        }
        Some(dir) => {
            let temp = tempfile::Builder::new()
                .prefix("martin-osm-nodes-")
                .tempdir_in(dir)
                .map_err(|error| OsmError::TempFile {
                    dir: dir.to_path_buf(),
                    error,
                })?;
            let mut store = PagedStore::open(temp.path().join("nodes")).map_err(|error| OsmError::NodeStore {
                path: path.to_path_buf(),
                error,
            })?;
            let count = file.load_nodes(&mut store).map_err(read_error)?;
            (
                Store::Paged {
                    store: Box::new(store),
                    _dir: temp,
                },
                count,
            )
        }
    };
    info!(
        osm.file = %path.display(),
        osm.nodes = count,
        elapsed = ?started.elapsed(),
        "Loaded node locations"
    );
    Ok(store)
}

impl FeatureSource for OsmSource {
    fn partitions(&self) -> u32 {
        u32::try_from(self.partitions.len()).expect("fewer than 2^32 partitions")
    }

    fn read(
        &self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
    ) -> SourceResult<()> {
        OsmReader::new(self).read(partition, keys, emit)
    }

    fn open_readers(&self, count: usize) -> SourceResult<Vec<Box<dyn FeatureReader + '_>>> {
        Ok((0..count.max(1))
            .map(|_| Box::new(OsmReader::new(self)) as Box<dyn FeatureReader + '_>)
            .collect())
    }

    /// Fails if a read found blocks whose string table goes on after their first group, which may have made the
    /// block filters skip objects a layer takes.
    fn finish(&self) -> SourceResult<()> {
        for scan in &self.scans {
            let blocks = scan.file.split_string_tables();
            if blocks > 0 {
                return Err(OsmError::SplitStringTables {
                    path: scan.path.clone(),
                    blocks,
                }
                .into());
            }
        }
        Ok(())
    }
}

/// One reader thread: the node lookups of each scan it reads ways of, kept from partition to partition.
struct OsmReader<'a> {
    source: &'a OsmSource,
    locators: Vec<Option<Locator<StoreReader<'a>>>>,
}

/// A thread's reader of one [`Store`].
enum StoreReader<'a> {
    /// Reads a [`MemoryStore`].
    Memory(MemoryReader<'a>),
    /// Reads a [`PagedStore`].
    Paged(PagedReader<'a>),
}

impl NodeReader for StoreReader<'_> {
    fn get(&mut self, id: u64) -> Option<Location> {
        match self {
            Self::Memory(reader) => reader.get(id),
            Self::Paged(reader) => reader.get(id),
        }
    }
}

impl<'a> OsmReader<'a> {
    /// A reader with a node lookup in each scan's store.
    fn new(source: &'a OsmSource) -> Self {
        let locators = source
            .scans
            .iter()
            .map(|scan| {
                scan.store.as_ref().map(|store| {
                    Locator::new(match &**store {
                        Store::Memory(store) => StoreReader::Memory(store.reader()),
                        Store::Paged { store, .. } => StoreReader::Paged(store.reader()),
                    })
                })
            })
            .collect();
        Self { source, locators }
    }
}

impl FeatureReader for OsmReader<'_> {
    fn read(
        &mut self,
        partition: u32,
        _keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
    ) -> SourceResult<()> {
        let (index, part) = self
            .source
            .partitions
            .get(partition as usize)
            .ok_or(SourceError::UnknownPartition(partition))?;
        let (index, part) = (*index, part.clone());
        let scan = &self.source.scans[usize::from(index)];
        let mut out = Batches {
            table: index,
            partition,
            next_row: 0,
            batch: Vec::with_capacity(BATCH_FEATURES),
            arena: BytesMut::new(),
            emit,
        };
        let mut missing = 0u64;
        let range = match part {
            Partition::Blocks(range) => range,
            Partition::Relations(relations) => {
                let lookup = self.locators[usize::from(index)]
                    .as_mut()
                    .expect("a scan with relations has a node store");
                read_relations_of(scan, &scan.relations[relations], lookup, &mut out)?;
                return out.flush();
            }
        };
        let blocks = scan
            .file
            .blocks_in(range, scan.select)
            .map_err(read_error(&scan.path))?;
        for block in blocks {
            let block = block.map_err(read_error(&scan.path))?;
            let keys = column_keys(&block, &scan.columns);
            read_nodes(scan, &block, &keys, &mut out)?;
            if let Some(lookup) = &mut self.locators[usize::from(index)] {
                missing += read_ways(scan, &block, &keys, lookup, &mut out)?;
            }
        }
        if missing > 0 {
            debug!(
                osm.partition = partition,
                osm.ways = missing,
                "Skipped ways with nodes missing from the file"
            );
        }
        out.flush()
    }
}

/// The features of one partition, sent on in batches.
struct Batches<'e> {
    table: u16,
    partition: u32,
    next_row: u64,
    batch: Vec<SourceFeature>,
    arena: BytesMut,
    emit: &'e mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
}

impl Batches<'_> {
    /// Adds `feature`, sending the batch on once it is full.
    fn push(&mut self, feature: SourceFeature) -> SourceResult<()> {
        self.batch.push(feature);
        if self.batch.len() >= BATCH_FEATURES {
            self.flush()?;
        }
        Ok(())
    }

    /// Sends on the features gathered so far, if any.
    fn flush(&mut self) -> SourceResult<()> {
        if self.batch.is_empty() {
            return Ok(());
        }
        let features = std::mem::replace(&mut self.batch, Vec::with_capacity(BATCH_FEATURES));
        let first_row = self.next_row;
        self.next_row += features.len() as u64;
        (self.emit)(FeatureBatch {
            table: self.table,
            partition: self.partition,
            first_row,
            crs: Crs::Wgs84,
            features,
        })
    }

    /// Copies `text` into the batch's shared buffer.
    fn str(&mut self, text: &str) -> SourceStr {
        if self.arena.capacity() < text.len() {
            self.arena.reserve(text.len().max(ARENA_BYTES));
        }
        self.arena.extend_from_slice(text.as_bytes());
        let bytes = self.arena.split().freeze();
        SourceStr::try_from(bytes).expect("copied from a str")
    }

    /// The scanned columns of an object as properties.
    fn props<'t>(
        &mut self,
        kind: Kind,
        columns: &[String],
        value: impl Fn(usize) -> Option<&'t str>,
    ) -> Vec<(KeyId, Prop<SourceStr>)> {
        let mut props = Vec::with_capacity(columns.len());
        for (pos, column) in (0u32..).zip(columns) {
            let text = if column == OSM_TYPE {
                Some(SourceStr::from_static(kind.name()))
            } else {
                value(pos as usize).map(|text| self.str(text))
            };
            if let Some(text) = text {
                props.push((KeyId::new(pos), Prop::Str(text)));
            }
        }
        props
    }
}

/// What an OSM object is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A node, read as a point.
    Node,
    /// A way, read as a line or an area.
    Way,
    /// A relation, read as an area.
    Relation,
}

impl Kind {
    /// Its value in the [`OSM_TYPE`] column.
    fn name(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Way => "way",
            Self::Relation => "relation",
        }
    }

    /// The feature id of the object of this kind with OSM id `id`: the id times 10, plus 1 for a node, 2 for a way
    /// and 3 for a relation. `None` for a negative id.
    fn feature_id(self, id: i64) -> Option<i64> {
        let suffix = match self {
            Self::Node => 1,
            Self::Way => 2,
            Self::Relation => 3,
        };
        (id >= 0).then_some(id)?.checked_mul(10)?.checked_add(suffix)
    }

    /// The kind and OSM id of the object with feature id `id`, the inverse of [`Self::feature_id`].
    fn of_feature(id: u64) -> Option<(Self, i64)> {
        let kind = match id % 10 {
            1 => Self::Node,
            2 => Self::Way,
            3 => Self::Relation,
            _ => return None,
        };
        Some((kind, i64::try_from(id / 10).ok()?))
    }
}

/// An object's tags as a [`Row`], through its block's keys of the scanned columns.
struct ObjectRow<'s, 't> {
    columns: &'s [String],
    kind: Kind,
    /// The value of each scanned column, by position.
    tag: &'s dyn Fn(usize) -> Option<&'t str>,
    geometry: GeometryType,
}

impl Row for ObjectRow<'_, '_> {
    fn column(&self, name: &str) -> Option<&str> {
        if name == OSM_TYPE {
            return Some(self.kind.name());
        }
        let pos = self.columns.iter().position(|c| c == name)?;
        (self.tag)(pos)
    }

    fn geometry(&self) -> GeometryType {
        self.geometry
    }
}

/// Reads the block's tagged nodes that the scan's condition takes, as points.
fn read_nodes(scan: &Scan, block: &Block, keys: &[Option<TagKey<'_>>], out: &mut Batches<'_>) -> SourceResult<()> {
    for node in block.nodes() {
        if node.tags().len() == 0 || !scan.wants(Kind::Node, node.id()) {
            continue;
        }
        let tag = |pos: usize| node.tag(keys[pos]?).filter(|v| !v.is_empty());
        if !scan.takes(Kind::Node, GeometryType::Point, &tag) {
            continue;
        }
        let coord = Coord {
            x: f64::from(node.decimicro_lon()) / 1e7,
            y: f64::from(node.decimicro_lat()) / 1e7,
        };
        if scan.bounds.is_some_and(|b| !b.intersects(&coord)) {
            continue;
        }
        let props = out.props(Kind::Node, &scan.columns, tag);
        out.push(SourceFeature {
            id: Kind::Node.feature_id(node.id()),
            geometry: Geometry::Point(coord),
            props,
        })?;
    }
    Ok(())
}

/// Reads the block's ways, each closed one as Planetiler does: a polygon unless `area=no`, and its outline unless
/// `area=yes`; returns how many were skipped for nodes missing from the file.
fn read_ways(
    scan: &Scan,
    block: &Block,
    keys: &[Option<TagKey<'_>>],
    lookup: &mut Locator<StoreReader<'_>>,
    out: &mut Batches<'_>,
) -> SourceResult<u64> {
    let mut missing = 0;
    let area_key = block.tag_key("area");
    for way in block.ways() {
        if way.tags().len() == 0 || !scan.wants(Kind::Way, way.id()) {
            continue;
        }
        let mut refs = way.refs();
        let count = refs.len();
        let first = refs.next();
        let last = refs.last().or(first);
        let closed = count > 1 && first == last;
        let area = area_key.and_then(|key| way.tag(key));
        let can_be_line = (!closed || area != Some("yes")) && count >= 2;
        let can_be_polygon = closed && area != Some("no") && count >= 4;
        let tag = |pos: usize| way.tag(keys[pos]?).filter(|v| !v.is_empty());
        let polygon = can_be_polygon && scan.takes(Kind::Way, GeometryType::Polygon, &tag);
        let line = can_be_line
            && if can_be_polygon {
                scan.takes_outline(&tag)
            } else {
                scan.takes(Kind::Way, GeometryType::Line, &tag)
            };
        if !polygon && !line {
            continue;
        }
        let ring = match lookup.line_string(&way) {
            Ok(ring) => ring,
            Err(GeometryError::MissingNode(_)) => {
                missing += 1;
                continue;
            }
            Err(GeometryError::TooFewNodes | GeometryError::NotClosed | GeometryError::NoRing) => continue,
        };
        if let Some(bounds) = scan.bounds
            && !ring.bounding_rect().is_some_and(|r| r.intersects(&bounds))
        {
            continue;
        }
        let id = Kind::Way.feature_id(way.id());
        let props = out.props(Kind::Way, &scan.columns, tag);
        match (polygon && ring.is_closed(), line) {
            (true, true) => {
                let area = Geometry::Polygon(Polygon::new(ring.clone(), Vec::new()));
                out.push(SourceFeature {
                    id,
                    geometry: area,
                    props: props.clone(),
                })?;
                out.push(SourceFeature {
                    id,
                    geometry: Geometry::Outline(ring),
                    props,
                })?;
            }
            (true, false) => out.push(SourceFeature {
                id,
                geometry: Geometry::Polygon(Polygon::new(ring, Vec::new())),
                props,
            })?,
            (false, true) => out.push(SourceFeature {
                id,
                geometry: if can_be_polygon {
                    Geometry::Outline(ring)
                } else {
                    Geometry::Line(ring)
                },
                props,
            })?,
            (false, false) => {}
        }
    }
    Ok(missing)
}

/// Assembles `relations` into polygons; a relation with a member way the file does not have, whose ways make no
/// closed ring, or that reaches nodes the file does not have, is left out.
fn read_relations_of(
    scan: &Scan,
    relations: &[Relation],
    lookup: &mut Locator<StoreReader<'_>>,
    out: &mut Batches<'_>,
) -> SourceResult<()> {
    let (mut incomplete, mut failed) = (0u64, 0u64);
    for relation in relations {
        let Some(ways) = relation
            .ways
            .iter()
            .map(|id| scan.member_ways.get(id).map(Vec::as_slice))
            .collect::<Option<Vec<_>>>()
        else {
            incomplete += 1;
            continue;
        };
        let Ok(area) = lookup.multi_polygon(ways) else {
            failed += 1;
            continue;
        };
        if let Some(bounds) = scan.bounds
            && !area.bounding_rect().is_some_and(|r| r.intersects(&bounds))
        {
            continue;
        }
        let mut polygons = area.0;
        let geometry = if polygons.len() == 1 {
            Geometry::Polygon(polygons.pop().expect("one polygon"))
        } else {
            Geometry::Polygons(polygons)
        };
        let props = out.props(Kind::Relation, &scan.columns, |pos| relation.values[pos].as_deref());
        out.push(SourceFeature {
            id: Kind::Relation.feature_id(relation.id),
            geometry,
            props,
        })?;
    }
    if incomplete > 0 {
        debug!(
            osm.relations = incomplete,
            "Skipped relations with member ways the file does not have"
        );
    }
    if failed > 0 {
        debug!(osm.relations = failed, "Skipped relations whose ways make no area");
    }
    Ok(())
}

impl fmt::Debug for OsmSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OsmSource")
            .field("partitions", &self.partitions.len())
            .finish_non_exhaustive()
    }
}
