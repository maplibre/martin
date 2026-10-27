//! Ramer-Douglas-Peucker for every zoom of a feature in one pass.

use std::iter::once;
use std::ops::Range;

use geo::Vector2DOps as _;
use geo_types::{Coord, LineString, Polygon};

/// A feature's lines or polygons with every vertex ranked by the largest tolerance RDP keeps it at,
/// so each zoom simplifies by filtering.
#[derive(Default)]
pub struct Simplifier {
    coords: Vec<Coord<f64>>,
    ranks: Vec<f64>,
    lines: Vec<Range<usize>>,
    polygons: Vec<Range<usize>>,
    stack: Vec<Span>,
}

/// A line section RDP still has to split, with the rank its split vertex may not exceed.
struct Span {
    first: usize,
    last: usize,
    cap: f64,
}

impl Simplifier {
    /// Ranks `lines` for every tolerance of at least `min_epsilon`.
    pub fn set_lines(&mut self, lines: &[LineString<f64>], min_epsilon: f64) {
        self.clear();
        for line in lines {
            self.push(line, min_epsilon * min_epsilon);
        }
    }

    /// Ranks the rings of `polygons` for every tolerance of at least `min_epsilon`.
    pub fn set_polygons(&mut self, polygons: &[Polygon<f64>], min_epsilon: f64) {
        self.clear();
        for polygon in polygons {
            let first_ring = self.lines.len();
            for ring in once(polygon.exterior()).chain(polygon.interiors()) {
                self.push(ring, min_epsilon * min_epsilon);
            }
            self.polygons.push(first_ring..self.lines.len());
        }
    }

    /// Each line as RDP simplifies it at `epsilon`.
    pub fn lines(&self, epsilon: f64) -> impl Iterator<Item = impl Iterator<Item = Coord<f64>>> {
        self.lines
            .iter()
            .map(move |line| self.simplified(line.clone(), epsilon))
    }

    /// Each polygon's rings, exterior first, as RDP simplifies them at `epsilon`.
    pub fn polygons(
        &self,
        epsilon: f64,
    ) -> impl Iterator<Item = impl Iterator<Item = impl Iterator<Item = Coord<f64>>>> {
        self.polygons.iter().map(move |rings| {
            self.lines[rings.clone()]
                .iter()
                .map(move |ring| self.simplified(ring.clone(), epsilon))
        })
    }

    /// The vertices of the `line` range that RDP keeps at `epsilon`.
    fn simplified(&self, line: Range<usize>, epsilon: f64) -> impl Iterator<Item = Coord<f64>> {
        let min_rank = epsilon * epsilon;
        self.coords[line.clone()]
            .iter()
            .zip(&self.ranks[line])
            .filter(move |&(_, &rank)| epsilon <= 0.0 || rank > min_rank)
            .map(|(&c, _)| c)
    }

    /// Drops the previous feature's geometry.
    fn clear(&mut self) {
        self.coords.clear();
        self.ranks.clear();
        self.lines.clear();
        self.polygons.clear();
    }

    /// Appends `line` and ranks its vertices by RDP down to the squared tolerance `min_rank`.
    fn push(&mut self, line: &LineString<f64>, min_rank: f64) {
        let start = self.coords.len();
        self.coords.extend_from_slice(&line.0);
        self.ranks.resize(self.coords.len(), 0.0);
        self.lines.push(start..self.coords.len());
        let (coords, ranks) = (&self.coords[start..], &mut self.ranks[start..]);
        let [first, .., last] = ranks else {
            if let [only] = ranks {
                *only = f64::INFINITY;
            }
            return;
        };
        *first = f64::INFINITY;
        *last = f64::INFINITY;

        self.stack.push(Span {
            first: 0,
            last: coords.len() - 1,
            cap: f64::INFINITY,
        });
        while let Some(Span { first, last, cap }) = self.stack.pop() {
            let (a, b) = (coords[first], coords[last]);
            let mut farthest = (first, 0.0);
            for (i, &c) in (first + 1..).zip(&coords[first + 1..last]) {
                let d = distance_sq(c, a, b);
                if d > farthest.1 {
                    farthest = (i, d);
                }
            }
            let (split, d) = farthest;
            if d > min_rank {
                let rank = d.min(cap);
                ranks[split] = rank;
                self.stack.push(Span {
                    first,
                    last: split,
                    cap: rank,
                });
                self.stack.push(Span {
                    first: split,
                    last,
                    cap: rank,
                });
            }
        }
    }
}

/// The squared distance from `c` to the segment from `a` to `b`.
fn distance_sq(c: Coord<f64>, a: Coord<f64>, b: Coord<f64>) -> f64 {
    let (ab, ac) = (b - a, c - a);
    let dot = ac.x.algebraic_mul(ab.x).algebraic_add(ac.y.algebraic_mul(ab.y));
    let len_sq = ab.magnitude_squared();
    if a == b || dot <= 0.0 {
        ac.magnitude_squared()
    } else if dot >= len_sq {
        (c - b).magnitude_squared()
    } else {
        let cross = ac.x.algebraic_mul(ab.y).algebraic_sub(ac.y.algebraic_mul(ab.x));
        cross.algebraic_mul(cross).algebraic_div(len_sq)
    }
}
