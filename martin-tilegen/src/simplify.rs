//! Douglas-Peucker over a flat slice: an explicit stack instead of recursion, squared distances instead
//! of `hypot`, and caller-owned buffers, so simplifying allocates nothing in the steady state.

use geo_types::Coord;

/// Reusable buffers for [`simplify`].
#[derive(Default)]
pub struct Simplifier {
    stack: Vec<(usize, usize)>,
}

impl Simplifier {
    /// Appends to `kept`, in order, the indices of the `points` Douglas-Peucker keeps at `epsilon`.
    /// Distances are to the chord segment and ties go to the later vertex, as in `geo::SimplifyIdx`.
    pub fn simplify(&mut self, points: &[Coord<f64>], epsilon: f64, kept: &mut Vec<usize>) {
        if points.len() < 3 || epsilon <= 0.0 {
            kept.extend(0..points.len());
            return;
        }
        let max_dist2 = epsilon * epsilon;
        self.stack.clear();
        self.stack.push((0, points.len() - 1));
        // Left halves are pushed last, so spans pop in vertex order and `kept` comes out sorted.
        while let Some((first, last)) = self.stack.pop() {
            let chord = Chord::new(points[first], points[last]);
            let (farthest, dist2) = points[first + 1..last]
                .iter()
                .enumerate()
                .map(|(i, &p)| (first + 1 + i, chord.dist2(p)))
                .fold(
                    (first, 0.0),
                    |best, cur| if cur.1 >= best.1 { cur } else { best },
                );
            if dist2 > max_dist2 {
                self.stack.push((farthest, last));
                self.stack.push((first, farthest));
            } else {
                kept.push(first);
            }
        }
        kept.push(points.len() - 1);
    }
}

struct Chord {
    start: Coord<f64>,
    dir: Coord<f64>,
    inv_len2: f64,
}

impl Chord {
    fn new(start: Coord<f64>, end: Coord<f64>) -> Self {
        let dir = end - start;
        let len2 = dir.x * dir.x + dir.y * dir.y;
        // A point-sized chord (a closed ring's first and last vertex) measures from its start.
        let inv_len2 = if len2 > 0.0 { 1.0 / len2 } else { 0.0 };
        Self {
            start,
            dir,
            inv_len2,
        }
    }

    /// Squared distance to the segment, branch-free so the scan loop vectorizes.
    fn dist2(&self, point: Coord<f64>) -> f64 {
        let rel = point - self.start;
        let along = ((rel.x * self.dir.x + rel.y * self.dir.y) * self.inv_len2).clamp(0.0, 1.0);
        let off = rel - self.dir * along;
        off.x * off.x + off.y * off.y
    }
}

#[cfg(test)]
mod tests {
    use geo::SimplifyIdx as _;
    use geo_types::LineString;

    use super::*;

    fn ours(points: &[Coord<f64>], epsilon: f64) -> Vec<usize> {
        let mut kept = Vec::new();
        Simplifier::default().simplify(points, epsilon, &mut kept);
        kept
    }

    #[test]
    fn matches_geo_on_random_lines_and_rings() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            f64::from(u32::try_from(state >> 40).unwrap()) / f64::from(1 << 24)
        };
        let mut simplifier = Simplifier::default();
        let mut kept = Vec::new();
        for case in 0..2000 {
            let n = 2 + case % 60;
            let mut points: Vec<_> = (0..n)
                .map(|_| Coord {
                    x: next(),
                    y: next(),
                })
                .collect();
            if case % 2 == 0 {
                points.push(points[0]);
            }
            let epsilon = [0.0, 0.01, 0.05, 0.2, 1.0][case % 5];
            kept.clear();
            simplifier.simplify(&points, epsilon, &mut kept);
            assert_eq!(
                kept,
                LineString::new(points.clone()).simplify_idx(epsilon),
                "case {case}"
            );
        }
    }

    #[test]
    fn keeps_short_lines_and_endpoints() {
        let c = |x, y| Coord { x, y };
        assert_eq!(ours(&[], 1.0), Vec::<usize>::new());
        assert_eq!(ours(&[c(0.0, 0.0)], 1.0), [0]);
        assert_eq!(ours(&[c(0.0, 0.0), c(5.0, 0.0)], 1.0), [0, 1]);
        let line = [c(0.0, 0.0), c(1.0, 1.4), c(2.0, 3.0), c(3.0, 0.0)];
        assert_eq!(ours(&line, 0.5), [0, 2, 3]);
        assert_eq!(ours(&line, 5.0), [0, 3]);
    }
}
