//! The spec file: one case per row, with what the layers make of it.

use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::generate::spec::backend::RowKey;
use crate::generate::{GenerateError, GenerateResult};

/// What a spec file holds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Spec {
    /// Every case renders zooms 0 to this.
    pub max_zoom: u8,
    pub cases: Vec<Case>,
}

/// The key column of each table source, which holds the key of a case's row in its `input`.
pub type IdColumns = BTreeMap<String, String>;

/// One row of a table source and what its layers make of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Case {
    pub source: String,
    /// The paths through the layers this row takes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// The row as bulk mode read it when the case was written: the columns the layers read, and the geometry as WKT
    /// in the coordinates of the scan, lon/lat unless the source is in Web Mercator.
    /// Its key column selects the row.
    pub input: BTreeMap<String, Value>,
    pub output: Vec<Output>,
    /// What each per-tile operation of the layers did to the row's features, over all its tiles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub per_tile: Vec<OpCount>,
}

impl Case {
    /// The first path the row takes, to tell the case apart.
    #[must_use]
    pub fn path(&self) -> &str {
        self.paths.first().map_or("", String::as_str)
    }

    /// The key of the row, the value of its source's key column in `input`.
    #[must_use]
    pub fn id(&self, ids: &IdColumns) -> Option<RowKey> {
        let column = ids.get(&self.source)?;
        self.input.get(column)?.as_u64()
    }

    /// Whether `other` is a case of the same row.
    #[must_use]
    pub fn same_row(&self, other: &Self, ids: &IdColumns) -> bool {
        self.source == other.source && self.id(ids).is_some_and(|id| other.id(ids) == Some(id))
    }
}

/// A case's name: its source and the id of its row.
pub struct CaseName<'a>(pub &'a Case, pub &'a IdColumns);

impl Display for CaseName<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let Self(case, ids) = *self;
        match case.id(ids) {
            Some(id) => write!(f, "{} {id}", case.source),
            None => write!(f, "{} (no id)", case.source),
        }
    }
}

/// One feature in one layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub layer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub geometry: GeometryKind,
    pub zooms: Zooms,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, Attribute>,
}

/// What one per-tile operation of one layer did to a case, summed over the tiles it ran on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpCount {
    /// The layer's name.
    pub layer: String,
    /// The operation's key in the config.
    pub op: String,
    /// Tiles of the layer the operation ran on.
    pub tiles: u64,
    /// Features the operation took, over those tiles.
    pub features_in: u64,
    /// Features the operation kept or made, over those tiles.
    pub features_out: u64,
}

/// The kind of a feature's geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GeometryKind {
    Point,
    Line,
    Polygon,
    Other,
}

impl Display for GeometryKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Point => "point",
            Self::Line => "line",
            Self::Polygon => "polygon",
            Self::Other => "other",
        })
    }
}

/// The zooms a feature is in its layer, written `min-max`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Zooms {
    pub min: u8,
    pub max: u8,
}

impl Display for Zooms {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.min, self.max)
    }
}

impl From<Zooms> for String {
    fn from(zooms: Zooms) -> Self {
        zooms.to_string()
    }
}

impl TryFrom<String> for Zooms {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.split_once('-')
            .and_then(|(min, max)| {
                Some(Self {
                    min: min.parse().ok()?,
                    max: max.parse().ok()?,
                })
            })
            .ok_or_else(|| format!("`{text}` is not `min-max` zooms"))
    }
}

/// An attribute's value, with its zooms where they are fewer than the feature's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Attribute {
    Zoomed {
        value: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        minzoom: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        maxzoom: Option<u8>,
    },
    Value(Value),
}

/// `value` as YAML, with each string on one line.
pub fn yaml(value: &impl Serialize) -> String {
    let options = serde_saphyr::ser_options! { prefer_block_scalars: false };
    serde_saphyr::to_string_with_options(value, options).expect("spec values serialize")
}

/// The features of a case, and what its per-tile operations did, as YAML, to diff.
#[must_use]
pub fn output_yaml(output: &[Output], per_tile: &[OpCount]) -> String {
    #[derive(Serialize)]
    struct Shown<'a> {
        output: &'a [Output],
        per_tile: &'a [OpCount],
    }
    match (output.is_empty(), per_tile.is_empty()) {
        (true, true) => "(no feature in any layer)\n".to_owned(),
        (false, true) => yaml(&output),
        (_, false) => yaml(&Shown { output, per_tile }),
    }
}

/// The input and features of a case, and what its per-tile operations did, as YAML, to diff.
#[must_use]
pub fn case_yaml(case: &Case) -> String {
    #[derive(Serialize)]
    struct Shown<'a> {
        input: &'a BTreeMap<String, Value>,
        output: &'a [Output],
        #[serde(skip_serializing_if = "<[OpCount]>::is_empty")]
        per_tile: &'a [OpCount],
    }
    yaml(&Shown {
        input: &case.input,
        output: &case.output,
        per_tile: &case.per_tile,
    })
}

/// Reads a spec file.
///
/// # Errors
///
/// If it cannot be read or parsed.
pub fn read_spec(path: &Path) -> GenerateResult<Spec> {
    let text = std::fs::read_to_string(path).map_err(|e| GenerateError::ReadSpec(path.to_path_buf(), e))?;
    serde_saphyr::from_str(&text).map_err(|e| GenerateError::ParseSpec(path.to_path_buf(), Box::new(e)))
}

/// Writes a spec file.
///
/// # Errors
///
/// If it cannot be written.
pub fn write_spec(path: &Path, spec: &Spec) -> GenerateResult<()> {
    std::fs::write(path, yaml(spec)).map_err(|e| GenerateError::WriteSpec(path.to_path_buf(), e))
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    #[test]
    fn a_long_geometry_is_written_on_one_line() {
        let spec: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: roads
                input:
                  osm_id: 1
                  geometry: LINESTRING(-84.480247 34.194863,-84.4801047 34.194814,-84.4800553 34.1947723,-84.4800081 34.1946969)
                output: []
        "})
        .expect("parses");
        insta::assert_snapshot!(yaml(&spec), @"
        max_zoom: 14
        cases:
        - source: roads
          input:
            geometry: LINESTRING(-84.480247 34.194863,-84.4801047 34.194814,-84.4800553 34.1947723,-84.4800081 34.1946969)
            osm_id: 1
          output: []
        ");
    }

    #[test]
    fn a_case_is_named_by_its_source_and_the_key_in_its_input() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let case: Case = serde_saphyr::from_str(indoc! {"
            source: roads
            input: { osm_id: 7, class: motorway }
            output: []
        "})
        .expect("parses");
        insta::assert_snapshot!(CaseName(&case, &ids), @"roads 7");
    }

    #[test]
    fn a_case_without_the_key_in_its_input_has_no_id() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let case: Case = serde_saphyr::from_str(indoc! {"
            source: roads
            input: { class: motorway }
            output: []
        "})
        .expect("parses");
        insta::assert_snapshot!(CaseName(&case, &ids), @"roads (no id)");
    }
}
