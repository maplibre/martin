//! Stateful reassembly of sliced tiles back into whole features — the accumulating inverse of the
//! [`Slicer`](crate::Slicer).
//!
//! [`Mosaic`] rejoins **polyline** tiles ([`Slicer::line`](crate::Slicer::line)); [`PolygonMosaic`] rejoins
//! **polygon** tiles ([`Slicer::polygon`](crate::Slicer::polygon)). Both take each tile's geometry
//! in its local frame, rebase it into the global frame (`local + tile·extent`), and link geometry
//! across borders by shared **directed edges**. [`add`](Mosaic::add) rejects a tile inconsistent with
//! those already present — naming the ones it conflicts with, and leaving the mosaic unchanged — in
//! two ways:
//!
//! - **Payload:** slicing duplicates a border-crossing segment identically (position **and** payload)
//!   in every tile that carries it, so a shared edge whose vertices *disagree* is a conflict. (A single
//!   position may be shared by several features — edges only collide when their whole segment does — so
//!   unrelated features meeting at a point never conflict.)
//! - **Membership:** every tile is complete in its own *core* (the cell it owns), so an edge with an
//!   endpoint in a present tile's core must be carried by that tile. A present tile that owns an
//!   endpoint but lacks the edge — a line spanning into a neighbor the neighbor never corroborates,
//!   or two tiles disagreeing at a shared junction — is a conflict. A buffer only widens the overlap
//!   two tiles share, and the whole overlap must match; any *disagreement* there surfaces here too,
//!   because the differing vertex lands in some tile's core. Only a pure *omission* of redundant
//!   overlap geometry goes unflagged — and that is harmless, since the core tile still supplies the
//!   edge and duplicates collapse on reassembly.
//!
//! [`iter_features`](Mosaic::iter_features) re-chains the distinct edges into maximal chains
//! (`stitch`) — the same reconstruction a from-scratch merge of all tiles would produce.
//!
//! The two mosaics differ only in how a tile's input becomes directed edges:
//!
//! - [`Mosaic`] uses every consecutive vertex pair of each **run**. It needs only the `extent`.
//! - [`PolygonMosaic`] takes each tile's **rings** and keeps only the edges that **touch** the tile's
//!   buffered box `B`. This drops all the slicer's synthetic geometry — the
//!   clip-boundary corners, fill boxes, and gap-bridging chords, which are routed strictly *outside*
//!   `B` — while every real ring edge survives in the tile that owns its start vertex (whose cell lies
//!   in `B`, so the edge touches `B` there). The *same* edge machinery then reassembles the rings, with
//!   orientation and holes falling out of the directed edges. Testing the edge rather than the vertex
//!   is what catches a bridge chord drawn directly between two *original* crossing vertices, which no
//!   per-vertex tag could. Because it re-derives `B`, it needs the `buffer` as well as the `extent`. A
//!   tile lying entirely inside a polygon (an all-synthetic fill) contributes no edges — correct, since
//!   a reassembled polygon's interior is implied by its winding, not by fill tiles.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use geo_types::Coord;

use crate::TileError;
use crate::clip_polyline::segment_intersects;
use crate::grid::Grid;
use crate::tile::{TileId, tile_of};
use crate::vertex::Vertex;

/// A global directed edge's two vertices (identical across every tile that carries it) and the set of
/// tiles that do.
#[derive(Debug, Clone)]
struct Edge<V> {
    a: V,
    b: V,
    tiles: BTreeSet<TileId>,
}

/// A directed edge keyed by its endpoint positions.
type EdgeKey = (Coord<i32>, Coord<i32>);

/// The shared global directed-edge index behind both mosaics: it holds the cross-tile edge map and
/// per-tile bookkeeping, detects conflicts, and re-chains features — everything except how a tile's
/// raw geometry is turned into directed edges (runs for [`Mosaic`], synthetic-filtered rings for
/// [`PolygonMosaic`]).
#[derive(Debug, Clone)]
struct EdgeIndex<V: Vertex> {
    /// Global directed-edge index, keyed by endpoint positions. Every tile at a key holds the same
    /// vertices (conflicts are rejected on commit).
    edges: HashMap<EdgeKey, Edge<V>>,
    /// The edge keys each tile contributed, for `O(tile)` purge and self-exclusion when re-adding.
    tile_edges: BTreeMap<TileId, Vec<EdgeKey>>,
}

impl<V: Vertex> EdgeIndex<V> {
    fn new() -> Self {
        Self {
            edges: HashMap::new(),
            tile_edges: BTreeMap::new(),
        }
    }

    /// Replace `tile`'s contribution with `new_edges` (its distinct directed edges, already rebased
    /// into the global frame), after rejecting any conflict with the other tiles. `ext` is the signed
    /// extent, used to map an endpoint to its owner tile for the membership check. On conflict nothing
    /// is mutated.
    fn commit(
        &mut self,
        tile: TileId,
        new_edges: HashMap<EdgeKey, (V, V)>,
        ext: i32,
    ) -> Result<(), TileError> {
        // Conflict scan against *other* tiles (this tile's own prior data is replaced, not compared).
        let mut conflicts = BTreeSet::new();
        // (a) Payload conflict: another tile carries the same edge (identical positions) but different
        // vertices — only a payload (e.g. an M value) can differ, since the key is the positions.
        for (key, (a, b)) in &new_edges {
            if let Some(existing) = self.edges.get(key)
                && !(existing.a == *a && existing.b == *b)
            {
                conflicts.extend(existing.tiles.iter().copied().filter(|&t| t != tile));
            }
        }
        // (b) Membership conflict: a tile is complete in its own core, so every edge with an endpoint
        // in a present tile's core must be carried by that tile. A present endpoint-tile that lacks the
        // edge means the tiles came from inconsistent data. Checked both ways so detection is
        // order-independent. (Any *disagreement* in a buffer overlap surfaces here too — the differing
        // vertex lands in some core — so only a harmless omission of a redundant overlap edge slips.)
        // This tile must carry every already-present edge that names it as an endpoint-tile.
        for (key, existing) in &self.edges {
            if (tile_of(key.0, ext) == tile || tile_of(key.1, ext) == tile)
                && !new_edges.contains_key(key)
            {
                conflicts.extend(existing.tiles.iter().copied().filter(|&t| t != tile));
            }
        }
        // Every already-present endpoint-tile of one of this tile's edges must carry that edge.
        for key in new_edges.keys() {
            for endpoint in [tile_of(key.0, ext), tile_of(key.1, ext)] {
                if endpoint != tile
                    && self.tile_edges.contains_key(&endpoint)
                    && !self
                        .edges
                        .get(key)
                        .is_some_and(|e| e.tiles.contains(&endpoint))
                {
                    conflicts.insert(endpoint);
                }
            }
        }
        if !conflicts.is_empty() {
            return Err(TileError::Conflict(conflicts.into_iter().collect()));
        }
        // Commit: replace any prior contents for this tile, then index the new edges.
        self.purge(tile);
        let mut keys = Vec::with_capacity(new_edges.len());
        for (key, (a, b)) in new_edges {
            self.edges
                .entry(key)
                .or_insert_with(|| Edge {
                    a,
                    b,
                    tiles: BTreeSet::new(),
                })
                .tiles
                .insert(tile);
            keys.push(key);
        }
        if !keys.is_empty() {
            self.tile_edges.insert(tile, keys);
        }
        Ok(())
    }

    /// Remove `tile` and all geometry it contributed. Returns whether the tile was present.
    fn purge(&mut self, tile: TileId) -> bool {
        let Some(keys) = self.tile_edges.remove(&tile) else {
            return false;
        };
        for key in keys {
            if let Some(edge) = self.edges.get_mut(&key) {
                edge.tiles.remove(&tile);
                if edge.tiles.is_empty() {
                    self.edges.remove(&key);
                }
            }
        }
        true
    }

    /// Re-chain the distinct edges into maximal chains, in the global frame (deterministic order).
    fn iter_features(&self) -> impl Iterator<Item = Vec<V>> {
        let mut ordered: Vec<&Edge<V>> = self.edges.values().collect();
        ordered.sort_unstable_by_key(|edge| {
            let (a, b) = (edge.a.position(), edge.b.position());
            (a.x, a.y, b.x, b.y)
        });
        let runs: Vec<Vec<V>> = ordered.iter().map(|edge| vec![edge.a, edge.b]).collect();
        stitch(&runs).into_iter()
    }

    fn contains(&self, tile: TileId) -> bool {
        self.tile_edges.contains_key(&tile)
    }

    fn len(&self) -> usize {
        self.tile_edges.len()
    }

    fn is_empty(&self) -> bool {
        self.tile_edges.is_empty()
    }

    fn clear(&mut self) {
        self.edges.clear();
        self.tile_edges.clear();
    }
}

/// Reassembles tiled **polyline** features back into whole features as tiles are [added](Self::add).
///
/// Generic over the [`Vertex`] type `V` (defaults to [`Coord<i32>`]). Needs only the `extent` the
/// tiles were sliced at — never the buffer (see the module docs). A payload-carrying vertex (e.g.
/// [`Measured`](crate::Measured)) additionally makes payload [`Conflict`](TileError::Conflict)s
/// meaningful.
#[derive(Debug, Clone)]
pub struct Mosaic<V: Vertex = Coord<i32>> {
    extent: u32,
    index: EdgeIndex<V>,
}

impl<V: Vertex> Mosaic<V> {
    /// Create an empty mosaic for tiles sliced at `extent`.
    ///
    /// # Errors
    ///
    /// [`TileError::InvalidExtent`] if `extent` is `0` or greater than `i32::MAX`.
    pub fn new(extent: u32) -> Result<Self, TileError> {
        if extent == 0 || extent > i32::MAX.cast_unsigned() {
            return Err(TileError::InvalidExtent);
        }
        Ok(Self {
            extent,
            index: EdgeIndex::new(),
        })
    }

    /// The extent every tile is sliced at.
    #[must_use]
    pub fn extent(&self) -> u32 {
        self.extent
    }

    /// Add `tile`'s runs (in its local frame) to the mosaic. `runs` is anything sliceable to `[V]`
    /// (e.g. `&[&[V]]` collected from a tile's features). Re-adding a tile replaces its contents. A
    /// tile with no ≥2-vertex run contributes nothing.
    ///
    /// # Errors
    ///
    /// - [`TileError::Conflict`] if the tile disagrees with already-added tiles on a shared border
    ///   segment. The mosaic is unchanged.
    /// - [`TileError::Overflow`] if a vertex overflows `i32` when rebased into the global frame.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn add<L: AsRef<[V]>>(&mut self, tile: TileId, runs: &[L]) -> Result<(), TileError> {
        let (off_x, off_y) = tile_offset(tile, self.extent);
        // Rebase to the global frame and collect this tile's distinct directed edges. Nothing is
        // mutated yet, so an overflow (or a later conflict) leaves the mosaic untouched.
        let mut new_edges: HashMap<EdgeKey, (V, V)> = HashMap::new();
        for run in runs {
            let g = rebase(run.as_ref(), off_x, off_y)?;
            for w in g.windows(2) {
                let key = (w[0].position(), w[1].position());
                if key.0 != key.1 {
                    new_edges.entry(key).or_insert((w[0], w[1]));
                }
            }
        }
        self.index
            .commit(tile, new_edges, self.extent.cast_signed())
    }

    /// Remove `tile` and all geometry it contributed. Returns whether the tile was present.
    pub fn purge(&mut self, tile: TileId) -> bool {
        self.index.purge(tile)
    }

    /// Iterate every reassembled feature — geometry stitched across tile borders into maximal
    /// polylines, in the global frame. A single iterator over the whole mosaic; deterministic order.
    pub fn iter_features(&self) -> impl Iterator<Item = Vec<V>> {
        self.index.iter_features()
    }

    /// Whether `tile` is currently in the mosaic.
    #[must_use]
    pub fn contains(&self, tile: TileId) -> bool {
        self.index.contains(tile)
    }

    /// The number of tiles currently in the mosaic.
    #[must_use]
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Whether no tile has been added yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Discard every tile, keeping the extent.
    pub fn clear(&mut self) {
        self.index.clear();
    }
}

/// Reassembles tiled **polygon** features back into whole rings as tiles are [added](Self::add) — the
/// accumulating inverse of the polygon slicer.
///
/// Generic over the [`Vertex`] type `V` (defaults to [`Coord<i32>`]). Needs the `extent` **and**
/// `buffer` the tiles were sliced at, so it can re-derive each tile's buffered box and recognise the
/// synthetic clip-boundary corners the slicer injected (see the module docs). Reassembly is by
/// directed edge, so each ring comes back with its original orientation; a payload-carrying vertex
/// additionally makes payload [`Conflict`](TileError::Conflict)s meaningful.
#[derive(Debug, Clone)]
pub struct PolygonMosaic<V: Vertex = Coord<i32>> {
    grid: Grid,
    index: EdgeIndex<V>,
}

impl<V: Vertex> PolygonMosaic<V> {
    /// Create an empty polygon mosaic for tiles sliced at `extent` with `buffer`.
    ///
    /// # Errors
    ///
    /// - [`TileError::InvalidExtent`] if `extent` is `0`, or `extent + buffer` exceeds `i32::MAX`.
    /// - [`TileError::BufferTooLarge`] if `buffer` is not strictly less than half the `extent`.
    pub fn new(extent: u32, buffer: u16) -> Result<Self, TileError> {
        Ok(Self {
            grid: Grid::new(extent, buffer)?,
            index: EdgeIndex::new(),
        })
    }

    /// The extent every tile is sliced at.
    #[must_use]
    pub fn extent(&self) -> u32 {
        self.grid.extent()
    }

    /// The buffer every tile is sliced with.
    #[must_use]
    pub fn buffer(&self) -> u16 {
        self.grid.buffer()
    }

    /// Add `tile`'s rings (in its local frame) to the mosaic. `rings` is anything sliceable to `[V]`
    /// (e.g. `&[&[V]]` collected from a tile's polygon features — exterior and hole rings alike, in any
    /// order). Each ring may be given open or closed. Re-adding a tile replaces its contents. A tile
    /// that contributes only synthetic geometry (a solid interior fill) records nothing.
    ///
    /// The synthetic clip-boundary corners are dropped geometrically before anything is indexed, so
    /// they never enter the edge map and can raise no [`Conflict`](TileError::Conflict).
    ///
    /// # Errors
    ///
    /// - [`TileError::Conflict`] if the tile disagrees with already-added tiles on a shared border
    ///   edge. The mosaic is unchanged.
    /// - [`TileError::Overflow`] if the tile's box or a vertex overflows `i32` in the global frame.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn add<L: AsRef<[V]>>(&mut self, tile: TileId, rings: &[L]) -> Result<(), TileError> {
        let (min, max) = self.grid.tile_buffered_bounds(tile)?;
        let ext = self.grid.extent();
        let (off_x, off_y) = tile_offset(tile, ext);
        // Rebase each ring, drop its synthetic corners, and collect the surviving original directed
        // edges. Nothing is mutated yet, so an overflow (or a later conflict) leaves the mosaic intact.
        let mut new_edges: HashMap<EdgeKey, (V, V)> = HashMap::new();
        for ring in rings {
            ring_edges(ring.as_ref(), off_x, off_y, min, max, &mut new_edges)?;
        }
        self.index.commit(tile, new_edges, ext.cast_signed())
    }

    /// Remove `tile` and all geometry it contributed. Returns whether the tile was present.
    pub fn purge(&mut self, tile: TileId) -> bool {
        self.index.purge(tile)
    }

    /// Iterate every reassembled ring — geometry stitched across tile borders into maximal chains, in
    /// the global frame. A single iterator over the whole mosaic; deterministic order.
    pub fn iter_features(&self) -> impl Iterator<Item = Vec<V>> {
        self.index.iter_features()
    }

    /// Whether `tile` is currently in the mosaic.
    #[must_use]
    pub fn contains(&self, tile: TileId) -> bool {
        self.index.contains(tile)
    }

    /// The number of tiles currently in the mosaic (all-synthetic fill tiles do not count).
    #[must_use]
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Whether no tile has contributed geometry yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Discard every tile, keeping the extent and buffer.
    pub fn clear(&mut self) {
        self.index.clear();
    }
}

/// The global-frame offset `(tile.x·extent, tile.y·extent)` in `i64` (both factors within `i32`, so
/// the product is below `2^62`).
fn tile_offset(tile: TileId, extent: u32) -> (i64, i64) {
    let e = i64::from(extent);
    (i64::from(tile.x) * e, i64::from(tile.y) * e)
}

/// Rebase one vertex by `(off_x, off_y)` in `i64` (the offset is `tile · extent`, below `2^62`, and a
/// vertex adds at most another `i32`, so the sum stays within `i64`), range-checking back into `i32`.
fn rebase_vertex<V: Vertex>(v: V, off_x: i64, off_y: i64) -> Result<V, TileError> {
    let p = v.position();
    Ok(v.with_position(Coord {
        x: i32::try_from(i64::from(p.x) + off_x).map_err(|_overflow| TileError::Overflow)?,
        y: i32::try_from(i64::from(p.y) + off_y).map_err(|_overflow| TileError::Overflow)?,
    }))
}

/// Rebase every vertex of `run` by `(off_x, off_y)`, range-checking each result back into `i32`.
fn rebase<V: Vertex>(run: &[V], off_x: i64, off_y: i64) -> Result<Vec<V>, TileError> {
    run.iter()
        .map(|&v| rebase_vertex(v, off_x, off_y))
        .collect()
}

/// Rebase one closed `ring` to the global frame and add to `new_edges` every directed edge that
/// **touches** the tile's buffered box `[min, max]`.
///
/// This one test drops all the slicer's synthetic geometry and reassembles the original rings, because
/// of a clean split:
///
/// - Every **real** ring edge `p → q` is emitted by the tile that owns `p`'s cell: `p` lies in that
///   tile's core (⊆ its box), so the edge starts inside the box and touches it. Thus no real edge is
///   ever lost — it survives in at least the one tile that owns its start.
/// - Every **synthetic** edge — the clip-boundary corners, fill boxes, and gap-bridging chords the
///   slicer injects — is routed strictly *outside* the box, so it never touches the box
///   and is dropped. This catches even a direct bridge chord between two *original* crossing vertices,
///   which a per-vertex "is this corner synthetic" test cannot (its endpoints are genuine vertices).
///
/// An edge that misses the box but *is* real is simply redundant here (its endpoints belong to other
/// tiles' cores, which re-emit it), so dropping it is harmless. The rule is purely tile-local, hence
/// order-independent.
fn ring_edges<V: Vertex>(
    ring: &[V],
    off_x: i64,
    off_y: i64,
    min: Coord<i32>,
    max: Coord<i32>,
    new_edges: &mut HashMap<EdgeKey, (V, V)>,
) -> Result<(), TileError> {
    // Rebase to the global frame and drop consecutive duplicate positions in one pass (matching the
    // slicer, so a zero-length edge never distorts the touch test) — no separate rebased copy.
    let mut pts: Vec<V> = Vec::with_capacity(ring.len());
    for &v in ring {
        let g = rebase_vertex(v, off_x, off_y)?;
        if pts.last().map(Vertex::position) != Some(g.position()) {
            pts.push(g);
        }
    }
    // Drop the repeated closing vertex.
    while pts.len() >= 2 && pts[0].position() == pts[pts.len() - 1].position() {
        pts.pop();
    }
    let m = pts.len();
    if m < 3 {
        return Ok(());
    }
    // Emit each cyclic edge that touches the tile's box; drop the rest (synthetic or redundant).
    for i in 0..m {
        let (p, q) = (pts[i], pts[(i + 1) % m]);
        if p.position() != q.position() && segment_intersects(p.position(), q.position(), min, max)
        {
            new_edges
                .entry((p.position(), q.position()))
                .or_insert((p, q));
        }
    }
    Ok(())
}

/// Rejoin overlapping runs into maximal polylines via their **directed edges** (consecutive vertex
/// pairs, keyed by position): a border-crossing segment is the same edge in both tiles, so keeping the
/// distinct edges drops every duplicate, and following each vertex to its outgoing edge re-chains them.
///
/// Order-independent and deterministic (edges/positions keep first-seen order). A simple polyline
/// reconstructs exactly; where the geometry revisits a position, some covering chain is produced
/// (deterministic but arbitrary).
fn stitch<V: Vertex>(runs: &[Vec<V>]) -> Vec<Vec<V>> {
    // Distinct directed edges, in first-seen order (dedup by endpoint positions).
    let mut seen: HashSet<(Coord<i32>, Coord<i32>)> = HashSet::new();
    let mut edges: Vec<(V, V)> = Vec::new();
    for run in runs {
        for w in run.windows(2) {
            if seen.insert((w[0].position(), w[1].position())) {
                edges.push((w[0], w[1]));
            }
        }
    }
    if edges.is_empty() {
        return Vec::new();
    }

    // Adjacency: each start position → its outgoing edge indices (FIFO = first-seen). Track in-degree
    // and first-seen position order so chain starts are found deterministically.
    let mut out: HashMap<Coord<i32>, VecDeque<usize>> = HashMap::new();
    let mut indeg: HashMap<Coord<i32>, usize> = HashMap::new();
    let mut points: Vec<Coord<i32>> = Vec::new();
    let mut known: HashSet<Coord<i32>> = HashSet::new();
    for (i, (p, q)) in edges.iter().enumerate() {
        let (pp, qp) = (p.position(), q.position());
        out.entry(pp).or_default().push_back(i);
        *indeg.entry(qp).or_insert(0) += 1;
        indeg.entry(pp).or_insert(0);
        for pt in [pp, qp] {
            if known.insert(pt) {
                points.push(pt);
            }
        }
    }

    let mut used = vec![false; edges.len()];
    let mut chains: Vec<Vec<V>> = Vec::new();

    // Phase 1: start a chain at each path source — a position with more outgoing than incoming edges
    // accounts for `outdeg − indeg` chain starts (the endpoints of open polylines).
    for &p in &points {
        let outdeg = out.get(&p).map_or(0, VecDeque::len);
        let ind = indeg.get(&p).copied().unwrap_or(0);
        for _ in ind..outdeg {
            if let Some(first) = next_unused(&mut out, p, &used) {
                chains.push(build_chain(first, &edges, &mut used, &mut out));
            }
        }
    }
    // Phase 2: whatever edges remain form closed loops; start each at its first unused edge.
    for i in 0..edges.len() {
        if !used[i] {
            chains.push(build_chain(i, &edges, &mut used, &mut out));
        }
    }
    chains
}

/// Pop and return the next not-yet-used outgoing edge index from position `p`, discarding used ones
/// from the front. `None` when `p` has no remaining unused outgoing edge. Used by [`stitch`].
fn next_unused(
    out: &mut HashMap<Coord<i32>, VecDeque<usize>>,
    p: Coord<i32>,
    used: &[bool],
) -> Option<usize> {
    let dq = out.get_mut(&p)?;
    while let Some(&i) = dq.front() {
        if used[i] {
            dq.pop_front();
        } else {
            return dq.pop_front();
        }
    }
    None
}

/// Follow outgoing edges from edge `first`, consuming each, until a position has no unused outgoing
/// edge; return the vertex chain `[start, …]`. Used by [`stitch`].
fn build_chain<V: Vertex>(
    first: usize,
    edges: &[(V, V)],
    used: &mut [bool],
    out: &mut HashMap<Coord<i32>, VecDeque<usize>>,
) -> Vec<V> {
    let mut chain = vec![edges[first].0];
    let mut cur = first;
    loop {
        used[cur] = true;
        let q = edges[cur].1;
        chain.push(q);
        match next_unused(out, q.position(), used) {
            Some(next) => cur = next,
            None => break,
        }
    }
    chain
}
