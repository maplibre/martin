//! Joins lines whose ends meet, as JTS's `LineMerger` does: two lines are joined where exactly their two
//! ends meet, and never where three or more do, so junctions stay. geo has no equivalent yet.

use std::collections::BTreeMap;

use crate::sort::record::Vertex;

/// The lines `parts` make when joined end to end, each in the direction its first part is walked. Lines are
/// walked from ends that are not plain joints first, in input order, then the closed loops that remain.
/// Parts with fewer than two distinct vertices are left out.
pub(crate) fn merge(parts: &[Vec<Vertex>]) -> Vec<Vec<Vertex>> {
    let parts: Vec<&[Vertex]> = parts
        .iter()
        .map(Vec::as_slice)
        .filter(|part| part.windows(2).any(|pair| pair[0] != pair[1]))
        .collect();
    let mut ends: BTreeMap<Vertex, Vec<(usize, End)>> = BTreeMap::new();
    for (index, part) in parts.iter().enumerate() {
        ends.entry(part[0]).or_default().push((index, End::Start));
        ends.entry(part[part.len() - 1]).or_default().push((index, End::Finish));
    }
    let joint = |vertex: &Vertex| ends.get(vertex).is_some_and(|at| at.len() == 2);
    let mut walked = vec![false; parts.len()];
    let mut merged = Vec::new();
    for index in 0..parts.len() {
        for end in [End::Start, End::Finish] {
            if !walked[index] && !joint(&end.of(parts[index])) {
                merged.push(walk(&parts, &ends, &mut walked, index, end));
            }
        }
    }
    for index in 0..parts.len() {
        if !walked[index] {
            merged.push(walk(&parts, &ends, &mut walked, index, End::Start));
        }
    }
    merged
}

/// One end of a part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum End {
    Start,
    Finish,
}

impl End {
    /// The vertex at this end of `part`.
    fn of(self, part: &[Vertex]) -> Vertex {
        match self {
            Self::Start => part[0],
            Self::Finish => part[part.len() - 1],
        }
    }

    /// The other end.
    fn opposite(self) -> Self {
        match self {
            Self::Start => Self::Finish,
            Self::Finish => Self::Start,
        }
    }
}

/// The line that starts at `from` of part `index` and continues through every joint where exactly one
/// other part not yet walked meets it.
fn walk(
    parts: &[&[Vertex]],
    ends: &BTreeMap<Vertex, Vec<(usize, End)>>,
    walked: &mut [bool],
    mut index: usize,
    mut from: End,
) -> Vec<Vertex> {
    let mut line: Vec<Vertex> = Vec::new();
    loop {
        walked[index] = true;
        let part = parts[index];
        let skip = usize::from(!line.is_empty());
        match from {
            End::Start => line.extend(part.iter().skip(skip)),
            End::Finish => line.extend(part.iter().rev().skip(skip)),
        }
        let at = from.opposite();
        let node = at.of(part);
        let Some(meeting) = ends.get(&node).filter(|meeting| meeting.len() == 2) else {
            return line;
        };
        let Some(&(next, next_end)) = meeting.iter().find(|&&entry| entry != (index, at)) else {
            return line;
        };
        if walked[next] {
            return line;
        }
        index = next;
        from = next_end;
    }
}
