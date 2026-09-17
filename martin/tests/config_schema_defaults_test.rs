//! A setting whose docs declare a default must annotate it, or the generated config reference renders a placeholder where that default belongs.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

const SCHEMA: &str = include_str!("../../schemas/config.json");

const DEFAULT_PHRASES: [&str; 4] = ["default:", "defaults to", "default to", "defaulting to"];

const MAX_COMPOSITE_DEPTH: u8 = 4;

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Node {
    #[serde(rename = "$ref")]
    reference: Option<String>,
    #[serde(rename = "$defs")]
    defs: BTreeMap<String, Self>,
    description: Option<String>,
    properties: BTreeMap<String, Self>,
    examples: Vec<Value>,
    default: Option<Value>,
    any_of: Vec<Self>,
    one_of: Vec<Self>,
}

type Defs = BTreeMap<String, Node>;

impl Node {
    fn resolve<'a>(&'a self, defs: &'a Defs) -> &'a Self {
        self.reference
            .as_deref()
            .and_then(|reference| reference.rsplit('/').next())
            .and_then(|name| defs.get(name))
            .unwrap_or(self)
    }

    fn variants(&self) -> impl Iterator<Item = &Self> {
        self.any_of.iter().chain(&self.one_of)
    }

    fn spells_out_a_value(&self) -> bool {
        !self.examples.is_empty() || self.default.as_ref().is_some_and(|d| !d.is_null())
    }

    fn is_annotated(&self, defs: &Defs) -> bool {
        self.spells_out_a_value()
            || self.resolve(defs).spells_out_a_value()
            || self
                .variants()
                .any(|variant| variant.resolve(defs).spells_out_a_value())
    }

    /// Struct-shaped settings render as a block of documented sub-settings, which answer for their own defaults.
    fn renders_as_a_block(&self, defs: &Defs, depth: u8) -> bool {
        let resolved = self.resolve(defs);
        !resolved.properties.is_empty()
            || depth > 0
                && resolved
                    .variants()
                    .any(|variant| variant.renders_as_a_block(defs, depth - 1))
    }

    fn declares_a_default(&self) -> bool {
        let Some(description) = self.description.as_deref().map(str::to_lowercase) else {
            return false;
        };
        DEFAULT_PHRASES
            .iter()
            .any(|phrase| description.contains(phrase))
    }
}

fn collect_offenders(node: &Node, owner: &str, defs: &Defs, out: &mut Vec<String>) {
    for (name, property) in &node.properties {
        if property.declares_a_default()
            && !property.is_annotated(defs)
            && !property.renders_as_a_block(defs, MAX_COMPOSITE_DEPTH)
        {
            out.push(format!("{owner}.{name}"));
        }
        collect_offenders(property, owner, defs, out);
    }
    for variant in node.variants() {
        collect_offenders(variant, owner, defs, out);
    }
}

#[test]
fn every_documented_default_is_annotated_in_the_schema() {
    let root: Node = serde_json::from_str(SCHEMA).expect("schemas/config.json is a valid schema");

    let mut offenders = Vec::new();
    collect_offenders(&root, "<config root>", &root.defs, &mut offenders);
    for (name, definition) in &root.defs {
        collect_offenders(definition, name, &root.defs, &mut offenders);
    }
    offenders.sort_unstable();
    offenders.dedup();

    assert!(
        offenders.is_empty(),
        "these settings document a default but never annotate it. Annotate each with \
         `#[cfg_attr(feature = \"unstable-schemas\", schemars(example = &<value>))]` \
         and re-run `just gen-schemas`:\n  {}",
        offenders.join("\n  ")
    );
}
