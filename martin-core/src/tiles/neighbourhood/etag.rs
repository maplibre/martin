//! Derives an etag for a stitched tile from its nine input tiles' etags.

use std::hash::Hash;

use xxhash_rust::xxh3::Xxh3;

use super::NEIGHBOURHOOD_LEN;

/// What one of the nine neighbourhood slots contributes to the etag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputEtag<'a> {
    /// The slot was read and the tile returned an etag.
    Tagged(&'a str),
    /// The slot was read but the tile has no etag, so the output can't be tagged either.
    Untagged,
    /// The slot had no tile and was edge-clamped from the centre.
    Clamped,
}

impl<'a> InputEtag<'a> {
    /// Classifies a slot from the etag of the tile read into it, if any.
    ///
    /// `None` means nothing was read; `Some("")` means read but untagged.
    #[must_use]
    pub fn from_slot(etag: Option<&'a str>) -> Self {
        match etag {
            None => Self::Clamped,
            Some("") => Self::Untagged,
            Some(etag) => Self::Tagged(etag),
        }
    }
}

/// Derives the etag for a tile baked from `inputs` with `fingerprint`.
///
/// `inputs` is the nine slots in row-major order.
/// Returns `None` if any slot is untagged, since the bake can't be reliably tagged.
#[must_use]
pub fn neighbourhood_etag(
    inputs: &[InputEtag<'_>; NEIGHBOURHOOD_LEN],
    fingerprint: &str,
) -> Option<u128> {
    if inputs.contains(&InputEtag::Untagged) {
        return None;
    }
    let mut hasher = Xxh3::new();
    fingerprint.hash(&mut hasher);
    inputs.hash(&mut hasher);
    Some(hasher.digest128())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> [InputEtag<'static>; NEIGHBOURHOOD_LEN] {
        ["a", "b", "c", "d", "e", "f", "g", "h", "i"].map(InputEtag::Tagged)
    }

    #[test]
    fn slots_classify_by_what_was_read() {
        assert_eq!(InputEtag::from_slot(None), InputEtag::Clamped);
        assert_eq!(InputEtag::from_slot(Some("")), InputEtag::Untagged);
        assert_eq!(InputEtag::from_slot(Some("x")), InputEtag::Tagged("x"));
    }

    #[test]
    fn a_parameter_change_changes_the_tag() {
        assert_ne!(
            neighbourhood_etag(&full(), "params"),
            neighbourhood_etag(&full(), "other-params")
        );
    }

    #[test]
    fn any_input_changing_changes_the_tag() {
        let baseline = neighbourhood_etag(&full(), "p");
        for slot in 0..NEIGHBOURHOOD_LEN {
            let mut inputs = full();
            inputs[slot] = InputEtag::Tagged("changed");
            assert_ne!(
                neighbourhood_etag(&inputs, "p"),
                baseline,
                "slot {slot} must affect the tag"
            );
        }
    }

    #[test]
    fn which_slot_is_clamped_changes_the_tag() {
        let mut missing_north = full();
        missing_north[1] = InputEtag::Clamped;
        let mut missing_south = full();
        missing_south[7] = InputEtag::Clamped;
        assert_ne!(
            neighbourhood_etag(&missing_north, "p"),
            neighbourhood_etag(&missing_south, "p")
        );
    }

    #[test]
    fn a_clamped_slot_is_not_the_same_as_a_present_one() {
        let mut clamped = full();
        clamped[0] = InputEtag::Clamped;
        assert_ne!(
            neighbourhood_etag(&clamped, "p"),
            neighbourhood_etag(&full(), "p")
        );
    }

    #[test]
    fn an_input_without_an_etag_suppresses_the_tag_entirely() {
        for slot in 0..NEIGHBOURHOOD_LEN {
            let mut inputs = full();
            inputs[slot] = InputEtag::Untagged;
            assert_eq!(
                neighbourhood_etag(&inputs, "p"),
                None,
                "slot {slot} without an etag must suppress the tag"
            );
        }
    }

    #[test]
    fn an_entirely_clamped_neighbourhood_still_has_a_tag() {
        let inputs = [InputEtag::Clamped; NEIGHBOURHOOD_LEN];
        assert!(neighbourhood_etag(&inputs, "p").is_some());
    }

    #[test]
    fn inputs_cannot_be_confused_by_concatenation() {
        let mut left = full();
        left[0] = InputEtag::Tagged("ab");
        left[1] = InputEtag::Tagged("c");
        let mut right = full();
        right[0] = InputEtag::Tagged("a");
        right[1] = InputEtag::Tagged("bc");
        assert_ne!(
            neighbourhood_etag(&left, "p"),
            neighbourhood_etag(&right, "p")
        );
    }
}
