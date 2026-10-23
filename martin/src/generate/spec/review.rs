//! The cases `martin test generate` made, against the spec, and the decisions on them.

use std::fmt::{self, Display};

use crate::generate::spec::file::{Case, CaseName, IdColumns, Spec, case_yaml, output_yaml};
use crate::generate::spec::report::{Change, Look, Status, Tone, tally};
use crate::generate::spec::rows::Found;

/// How a generated case relates to the spec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseStatus {
    New,
    Changed,
    Same,
    Gone,
}

impl CaseStatus {
    /// The spec has no case of the row.
    const NEW: Look = Look::new("NEW", Tone::Cyan, "new", false);
    /// The spec's case of the row has another input or output.
    const CHANGED: Look = Look::new("CHANGED", Tone::Yellow, "changed", true);
    /// The spec's case of the row has the same input and output.
    const SAME: Look = Look::new("SAME", Tone::Green, "same", false);
    /// No path takes the spec's case any more.
    const GONE: Look = Look::new("GONE", Tone::Red, "gone", true);
}

impl Status for CaseStatus {
    const ALL: &'static [Look] = &[Self::NEW, Self::CHANGED, Self::SAME, Self::GONE];

    fn look(&self) -> Look {
        match self {
            Self::New => Self::NEW,
            Self::Changed => Self::CHANGED,
            Self::Same => Self::SAME,
            Self::Gone => Self::GONE,
        }
    }
}

/// Whether the spec takes a generated case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Undecided,
    Accepted,
    Rejected,
}

/// A generated case and the decision on it.
#[derive(Clone, Debug)]
pub struct Reviewed {
    pub case: Case,
    pub old: Option<Case>,
    pub status: CaseStatus,
    pub decision: Decision,
}

impl Reviewed {
    /// `case` against the case of the same row in `spec`.
    #[must_use]
    pub fn new(case: Case, spec: Option<&Spec>, ids: &IdColumns) -> Self {
        let old = spec
            .and_then(|spec| spec.cases.iter().find(|o| o.same_row(&case, ids)))
            .cloned();
        let status = match &old {
            None => CaseStatus::New,
            Some(old) if old.input == case.input && old.output == case.output => CaseStatus::Same,
            Some(_) => CaseStatus::Changed,
        };
        Self {
            case,
            old,
            status,
            decision: Decision::Undecided,
        }
    }

    /// The spec's output and the generated one as YAML, with their inputs where those differ.
    #[must_use]
    pub fn diff(&self) -> Change {
        let gone = self.status == CaseStatus::Gone;
        let input_changed = !gone && self.old.as_ref().is_some_and(|old| old.input != self.case.input);
        let shown = |case: &Case| {
            if input_changed {
                case_yaml(case)
            } else {
                output_yaml(&case.output)
            }
        };
        Change {
            old: self
                .old
                .as_ref()
                .map_or_else(|| "(not in the spec)\n".to_owned(), shown),
            new: if gone {
                "(no path takes the row any more)\n".to_owned()
            } else {
                shown(&self.case)
            },
        }
    }
}

/// The generated cases against the spec, and the decisions on them.
pub struct Review {
    pub found: Vec<Found>,
    pub cases: Vec<Reviewed>,
    max_zoom: u8,
    ids: IdColumns,
}

impl Review {
    /// The `cases` generated from `found`, with the cases of `spec` that no path takes any more.
    #[must_use]
    pub fn new(found: Vec<Found>, mut cases: Vec<Reviewed>, spec: Option<&Spec>, max_zoom: u8, ids: IdColumns) -> Self {
        for old in spec.iter().flat_map(|spec| &spec.cases) {
            if !cases.iter().any(|r| r.case.same_row(old, &ids)) {
                cases.push(Reviewed {
                    case: old.clone(),
                    old: Some(old.clone()),
                    status: CaseStatus::Gone,
                    decision: Decision::Undecided,
                });
            }
        }
        Self {
            found,
            cases,
            max_zoom,
            ids,
        }
    }

    /// The name of `case`.
    #[must_use]
    pub fn name<'a>(&'a self, case: &'a Case) -> CaseName<'a> {
        CaseName(case, &self.ids)
    }

    /// The cases to decide on, which are all but the unchanged.
    pub fn pending(&self) -> impl Iterator<Item = &Reviewed> {
        self.cases.iter().filter(|r| r.status != CaseStatus::Same)
    }

    /// The cases of the spec no path takes any more.
    pub fn gone(&self) -> impl Iterator<Item = &Reviewed> {
        self.cases.iter().filter(|r| r.status == CaseStatus::Gone)
    }

    /// Accepts every undecided case.
    pub fn accept_all(&mut self) {
        for r in &mut self.cases {
            if r.decision == Decision::Undecided {
                r.decision = Decision::Accepted;
            }
        }
    }

    /// The spec to write, with unchanged and accepted cases as generated and the others as the spec has them.
    #[must_use]
    pub fn spec(&self) -> Spec {
        let cases = self
            .cases
            .iter()
            .filter_map(|r| match (r.status, r.decision) {
                (CaseStatus::Gone, Decision::Accepted) => None,
                (CaseStatus::Same, _) | (_, Decision::Accepted) => Some(r.case.clone()),
                (_, Decision::Rejected | Decision::Undecided) => r.old.clone(),
            })
            .collect();
        Spec {
            max_zoom: self.max_zoom,
            cases,
        }
    }

    /// The paths and cases, counted by status.
    #[must_use]
    pub fn summary(&self) -> impl Display + '_ {
        fmt::from_fn(|f| {
            write!(
                f,
                "{} paths: {}, {} cases: {}",
                self.found.len(),
                tally(self.found.iter().map(|found| &found.row)),
                self.cases.len(),
                tally(self.cases.iter().map(|r| &r.status))
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::generate::spec::file::yaml;

    #[test]
    fn accepting_every_case_writes_the_generated_ones_and_drops_the_gone() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let old: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: roads
                input: { osm_id: 1 }
                output:
                  - { layer: roads, id: 1, geometry: line, zooms: 4-14 }
              - source: roads
                input: { osm_id: 9 }
                output: []
        "})
        .expect("parses");
        let generated: Vec<Case> = serde_saphyr::from_str(indoc! {"
            - source: roads
              input: { osm_id: 1 }
              output:
                - { layer: roads, id: 1, geometry: line, zooms: 5-14 }
            - source: roads
              input: { osm_id: 2 }
              output: []
        "})
        .expect("parses");
        let cases = generated
            .into_iter()
            .map(|case| Reviewed::new(case, Some(&old), &ids))
            .collect();
        let mut review = Review::new(Vec::new(), cases, Some(&old), 14, ids);
        review.accept_all();
        insta::assert_snapshot!(yaml(&review.spec()), @"
        max_zoom: 14
        cases:
        - source: roads
          input:
            osm_id: 1
          output:
          - layer: roads
            id: 1
            geometry: line
            zooms: 5-14
        - source: roads
          input:
            osm_id: 2
          output: []
        ");
    }

    #[test]
    fn a_case_of_a_row_the_spec_lacks_is_new_and_the_spec_case_of_no_row_found_is_gone() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let old: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: roads
                input: { osm_id: 1 }
                output:
                  - { layer: roads, id: 1, geometry: line, zooms: 4-14 }
        "})
        .expect("parses");
        let case: Case = serde_saphyr::from_str(indoc! {"
            source: roads
            input: { osm_id: 2 }
            output:
              - { layer: roads, id: 2, geometry: line, zooms: 4-14 }
        "})
        .expect("parses");
        let review = Review::new(
            Vec::new(),
            vec![Reviewed::new(case, Some(&old), &ids)],
            Some(&old),
            14,
            ids,
        );
        let statuses: Vec<(String, CaseStatus)> = review
            .cases
            .iter()
            .map(|r| (review.name(&r.case).to_string(), r.status))
            .collect();
        insta::assert_debug_snapshot!(statuses, @r#"
        [
            (
                "roads 2",
                New,
            ),
            (
                "roads 1",
                Gone,
            ),
        ]
        "#);
    }

    #[test]
    fn a_case_with_the_output_of_the_spec_is_same() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let old: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: roads
                input: { osm_id: 1 }
                output:
                  - { layer: roads, id: 1, geometry: line, zooms: 4-14 }
        "})
        .expect("parses");
        let case: Case = serde_saphyr::from_str(indoc! {"
            source: roads
            input: { osm_id: 1 }
            output:
              - { layer: roads, id: 1, geometry: line, zooms: 4-14 }
        "})
        .expect("parses");
        let review = Review::new(
            Vec::new(),
            vec![Reviewed::new(case, Some(&old), &ids)],
            Some(&old),
            14,
            ids,
        );
        let statuses: Vec<(String, CaseStatus)> = review
            .cases
            .iter()
            .map(|r| (review.name(&r.case).to_string(), r.status))
            .collect();
        insta::assert_debug_snapshot!(statuses, @r#"
        [
            (
                "roads 1",
                Same,
            ),
        ]
        "#);
    }

    #[test]
    fn a_case_with_another_output_than_the_spec_is_changed() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let old: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: roads
                input: { osm_id: 1 }
                output:
                  - { layer: roads, id: 1, geometry: line, zooms: 4-14 }
        "})
        .expect("parses");
        let case: Case = serde_saphyr::from_str(indoc! {"
            source: roads
            input: { osm_id: 1 }
            output:
              - { layer: roads, id: 1, geometry: line, zooms: 5-14 }
        "})
        .expect("parses");
        let review = Review::new(
            Vec::new(),
            vec![Reviewed::new(case, Some(&old), &ids)],
            Some(&old),
            14,
            ids,
        );
        let statuses: Vec<(String, CaseStatus)> = review
            .cases
            .iter()
            .map(|r| (review.name(&r.case).to_string(), r.status))
            .collect();
        insta::assert_debug_snapshot!(statuses, @r#"
        [
            (
                "roads 1",
                Changed,
            ),
        ]
        "#);
    }

    #[test]
    fn a_case_with_another_input_than_the_spec_is_changed_and_shows_both_inputs() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let old: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: roads
                input: { osm_id: 1, geometry: 'POINT(1 1)' }
                output:
                  - { layer: roads, id: 1, geometry: point, zooms: 4-14 }
        "})
        .expect("parses");
        let case: Case = serde_saphyr::from_str(indoc! {"
            source: roads
            input: { osm_id: 1, geometry: 'POINT(2 2)' }
            output:
              - { layer: roads, id: 1, geometry: point, zooms: 4-14 }
        "})
        .expect("parses");
        let reviewed = Reviewed::new(case, Some(&old), &ids);
        let diff = reviewed.diff();
        insta::assert_snapshot!(format!("{:?}\n--- spec\n{}--- now\n{}", reviewed.status, diff.old, diff.new), @"
        Changed
        --- spec
        input:
          geometry: POINT(1 1)
          osm_id: 1
        output:
        - layer: roads
          id: 1
          geometry: point
          zooms: 4-14
        --- now
        input:
          geometry: POINT(2 2)
          osm_id: 1
        output:
        - layer: roads
          id: 1
          geometry: point
          zooms: 4-14
        ");
    }
}
