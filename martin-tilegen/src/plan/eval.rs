//! Evaluates a [`Plan`] for each feature: which layers take it, and as what.

use std::borrow::Cow;
use std::fmt::{self, Debug, Formatter};
use std::ops::RangeInclusive;
use std::sync::atomic::Ordering;

use crate::props::{KeyId, KeyInterner, KeyNames, Prop, SourceStr};
use crate::source::SourceFeature;
use tracing::warn;

use super::{
    AttributesDef, Case, Plan, PlannedAttr, PlannedAttrs, PlannedId, PlannedLayer, PlannedTable, PlannedValue,
    PlannedZoom,
};
use crate::expr::{ExprKeys, ExprRef, FeatureView, PropSlots, Role};
use crate::sort::record::EncodedProps;
use crate::{Feature, FeatureGeom, RenderLayer, TileGenError, TileGenResult};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExprErrors {
    pub layer: String,
    pub expr: String,
    pub errors: u64,
}

/// Failed evaluations of each expression of a plan.
#[derive(Clone, Debug, Default)]
pub struct ExprTallies(Vec<u64>);

impl ExprTallies {
    pub fn merge(&mut self, other: Self) {
        if self.0.len() < other.0.len() {
            self.0.resize(other.0.len(), 0);
        }
        for (total, errors) in self.0.iter_mut().zip(other.0) {
            *total += errors;
        }
    }
}

/// Property keys as sources intern them, per table, and as tiles hold them, per output layer.
#[derive(Debug)]
pub(super) struct Keys {
    tables: Vec<KeyInterner>,
    /// What expressions know of each table's keys before any feature is read.
    names: Vec<(ExprKeys, u32)>,
    layers: Vec<KeyInterner>,
}

impl Keys {
    pub(super) fn new(tables: &[PlannedTable], layers: &[PlannedLayer]) -> Self {
        let keys: Vec<_> = tables
            .iter()
            .map(|table| {
                let keys = KeyInterner::new(&table.columns);
                for name in &table.reads {
                    keys.intern(name);
                }
                keys
            })
            .collect();
        Self {
            names: tables
                .iter()
                .zip(&keys)
                .map(|(table, keys)| {
                    let names = ExprKeys::columns(&table.columns);
                    let mut names = (names, key_count(table.columns.len()));
                    learn_all(&mut names, keys);
                    names
                })
                .collect(),
            tables: keys,
            layers: layers.iter().map(|layer| KeyInterner::new(&layer.keys)).collect(),
        }
    }
}

impl Plan {
    /// One per worker.
    #[must_use]
    pub fn evaluator(&self) -> Evaluator<'_> {
        Evaluator {
            plan: self,
            props: EncodedProps::default(),
            bands: Vec::new(),
            sort: Vec::new(),
            dynamic: vec![Vec::new(); self.layers.len()],
            slots: PropSlots::default(),
            names: self.keys.names.clone(),
            errors: vec![0; self.exprs.len()],
        }
    }

    /// The keys of each table, which its source interns the keys it finds into.
    #[must_use]
    pub fn table_keys(&self) -> &[KeyInterner] {
        &self.keys.tables
    }

    /// The expressions that failed on some features, which then took them as `null`, in plan order.
    #[must_use]
    pub fn expr_errors(&self, tallies: &ExprTallies) -> Vec<ExprErrors> {
        self.exprs
            .iter()
            .zip(&tallies.0)
            .filter(|(_, errors)| **errors > 0)
            .map(|(expr, &errors)| ExprErrors {
                layer: self.layers[expr.layer].info.name.clone(),
                expr: expr.source.clone(),
                errors,
            })
            .collect()
    }

    /// The keys of each output layer. Call once no more keys can appear, i.e. after rendering.
    pub(crate) fn key_names(&self) -> Vec<KeyNames> {
        self.keys.layers.iter().map(KeyInterner::names).collect()
    }

    /// A failed evaluation or conversion is counted, logged the first time, and counts as `null`.
    fn eval<R: Role>(&self, expr: &ExprRef<R>, view: &FeatureView<'_, SourceStr>, errors: &mut [u64]) -> R::Out {
        let index = expr.index;
        let planned = &self.exprs[index];
        match planned.expr.eval(view).and_then(|v| expr.role.coerce(v)) {
            Ok(value) => value,
            Err(err) => {
                errors[index] += 1;
                if !self.warned[index].swap(true, Ordering::Relaxed) {
                    warn!(
                        "layer `{}`: `{}` failed and counts as null: {err}",
                        self.layers[planned.layer].info.name, planned.source
                    );
                }
                R::Out::default()
            }
        }
    }

    /// The feature's zooms and the first case it matches, or `None` when the layer filters the feature
    /// out.
    fn feature_case<'p>(
        &self,
        layer: &'p PlannedLayer,
        view: &FeatureView<'_, SourceStr>,
        errors: &mut [u64],
    ) -> Option<(RangeInclusive<u8>, &'p Case)> {
        if let Some(filter) = &layer.filter
            && !self.eval(filter, view, errors)
        {
            return None;
        }
        let case = layer
            .cases
            .iter()
            .find(|case| case.when.as_ref().is_none_or(|when| self.eval(when, view, errors)))
            .unwrap_or(&layer.fallback);
        let (lo, hi) = (*layer.zooms.start(), *layer.zooms.end());
        let min = self.zoom(case.minzoom, view, errors).map_or(lo, |z| z.max(lo));
        let max = self.zoom(case.maxzoom, view, errors).map_or(hi, |z| z.min(hi));
        (min <= max).then_some((min..=max, case))
    }

    fn zoom(&self, zoom: PlannedZoom, view: &FeatureView<'_, SourceStr>, errors: &mut [u64]) -> Option<u8> {
        match zoom {
            PlannedZoom::Fixed(zoom) => Some(zoom),
            PlannedZoom::Expr(expr, fallback) => self
                .eval(&expr, view, errors)
                .or_else(|| self.eval(&fallback?, view, errors)),
        }
    }

    /// Adds the computed attributes to `props`, and fills `bands` if they have zoom bands.
    fn computed(
        &self,
        attrs: &PlannedAttrs,
        feature: &[(KeyId, Prop<SourceStr>)],
        view: Option<&FeatureView<'_, SourceStr>>,
        errors: &mut [u64],
        props: &mut EncodedProps,
        bands: &mut Vec<(u8, EncodedProps)>,
    ) {
        for attr in &attrs.computed {
            if let Some(value) = self.attr(attr, feature, view, errors) {
                props.push(attr.key, value.as_ref());
            }
        }
        if attrs.bands.is_empty() {
            return;
        }
        bands.resize_with(attrs.bands.len(), Default::default);
        for (band, &from) in bands.iter_mut().zip(&attrs.bands) {
            band.0 = from;
            band.1.copy_from(props);
        }
        for attr in &attrs.banded {
            let Some(value) = self.attr(attr, feature, view, errors) else {
                continue;
            };
            for (from, band) in bands.iter_mut() {
                if attr.zooms.contains(from) {
                    band.push(attr.key, value.as_ref());
                }
            }
        }
    }

    /// The layer's `sort_by` keys, concatenated; a failed one sorts as `null`.
    fn sort_key(&self, layer: &PlannedLayer, view: &FeatureView<'_, SourceStr>, errors: &mut [u64], out: &mut Vec<u8>) {
        out.clear();
        for expr in &layer.sort_by {
            self.eval(expr, view, errors).encode(expr.role.descending, out);
        }
    }

    fn attr<'v>(
        &self,
        attr: &'v PlannedAttr,
        feature: &'v [(KeyId, Prop<SourceStr>)],
        view: Option<&'v FeatureView<'_, SourceStr>>,
        errors: &mut [u64],
    ) -> Option<Prop<Cow<'v, str>>> {
        match &attr.value {
            PlannedValue::Literal(value) => Some(borrowed(value)),
            PlannedValue::Column(key) => feature.iter().find(|(k, _)| k == key).map(|(_, value)| borrowed(value)),
            PlannedValue::Expr(expr) => self.eval(expr, view?, errors),
        }
    }
}

/// Evaluates features for one worker, reusing its buffers from feature to feature.
pub struct Evaluator<'p> {
    plan: &'p Plan,
    props: EncodedProps,
    /// The props of each zoom band of the current layer, if it has bands.
    bands: Vec<(u8, EncodedProps)>,
    /// The `sort_by` key of the current feature in the current layer.
    sort: Vec<u8>,
    /// Output key of each table key interned after the declared columns, by layer; filled lazily.
    dynamic: Vec<Vec<DynamicKey>>,
    slots: PropSlots,
    /// Key names expressions read, by table, and how many of the table's keys they hold.
    names: Vec<(ExprKeys, u32)>,
    /// Failed evaluations by expression of the plan.
    errors: Vec<u64>,
}

impl Evaluator<'_> {
    /// Passes `emit` the feature as each layer of table `table` takes it, in layer order. `geom` is the
    /// feature's geometry, projected.
    ///
    /// # Errors
    ///
    /// If the plan has no table `table`, or `emit` fails.
    pub fn evaluate(
        &mut self,
        table: u16,
        feature: &SourceFeature,
        geom: FeatureGeom<'_>,
        mut emit: impl FnMut(&LayerFeature<'_>) -> TileGenResult<()>,
    ) -> TileGenResult<()> {
        let Self {
            plan,
            props,
            bands,
            sort,
            dynamic,
            slots,
            names,
            errors,
        } = self;
        let plan: &Plan = plan;
        let index = usize::from(table);
        let planned = plan.tables.get(index).ok_or(TileGenError::UnknownTable(table))?;
        if planned.evaluates && planned.dynamic_props {
            learn_keys(&mut names[index], &feature.props, &plan.keys.tables[index]);
        }
        let view = planned.evaluates.then(|| slots.bind(&names[index].0, &feature.props));
        let view = view.as_ref();
        for layer_index in planned.layers.clone() {
            let layer = &plan.layers[layer_index];
            if layer.geometry.is_some_and(|kind| !kind.matches(geom)) {
                continue;
            }
            let (zooms, case) = match view {
                Some(view) => match plan.feature_case(layer, view, errors) {
                    Some(matched) => matched,
                    None => continue,
                },
                None => (layer.zooms.clone(), &layer.fallback),
            };
            let attrs = &case.attrs;
            props.clear();
            for (key, value) in &feature.props {
                let output = (&plan.keys.tables[index], &plan.keys.layers[layer_index]);
                if let Some(key) = output_key(&mut dynamic[layer_index], layer, output, *key)
                    && (attrs.overrides.is_empty() || attrs.overrides.binary_search(&key).is_err())
                {
                    props.push(key, value.as_ref());
                }
            }
            plan.computed(attrs, &feature.props, view, errors, props, bands);
            let id = match layer.id {
                PlannedId::Keep => feature.id,
                PlannedId::Drop => None,
                PlannedId::Expr(expr) => view.and_then(|view| plan.eval(&expr, view, errors)),
            };
            let sort_key = match view {
                Some(view) if !layer.sort_by.is_empty() => {
                    plan.sort_key(layer, view, errors, sort);
                    Some(sort.as_slice())
                }
                _ => None,
            };
            let (props, bands): (&EncodedProps, &[(u8, EncodedProps)]) = match bands.split_first() {
                Some((first, rest)) if !attrs.bands.is_empty() => (&first.1, rest),
                _ => (props, &[]),
            };
            emit(&LayerFeature {
                plan,
                layer,
                feature: Feature {
                    id,
                    geom,
                    props,
                    bands,
                    sort: sort_key,
                    zooms,
                    simplify: case.simplify,
                    min_size: case.min_size,
                },
            })?;
        }
        Ok(())
    }

    #[must_use]
    pub fn finish(self) -> ExprTallies {
        ExprTallies(self.errors)
    }
}

/// A feature as one layer takes it.
pub struct LayerFeature<'a> {
    plan: &'a Plan,
    layer: &'a PlannedLayer,
    feature: Feature<'a>,
}

impl<'a> LayerFeature<'a> {
    #[must_use]
    pub fn layer(&self) -> &RenderLayer {
        &self.layer.render
    }

    #[must_use]
    pub fn feature(&self) -> &Feature<'a> {
        &self.feature
    }
}

impl Debug for LayerFeature<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let names = &self.plan.keys.layers[usize::from(self.layer.render.index)];
        let Feature {
            id,
            geom: _,
            props,
            bands,
            sort,
            zooms,
            simplify,
            min_size,
        } = &self.feature;
        let bands: Vec<_> = bands
            .iter()
            .map(|(zoom, props)| (zoom, NamedProps { props, names }))
            .collect();
        f.debug_struct("LayerFeature")
            .field("layer", &self.layer.info.name)
            .field("id", id)
            .field("zooms", zooms)
            .field("simplify", simplify)
            .field("min_size", min_size)
            .field("props", &NamedProps { props, names })
            .field("bands", &bands)
            .field("sort", &sort.map(Hex))
            .finish()
    }
}

struct NamedProps<'a> {
    props: &'a EncodedProps,
    names: &'a KeyInterner,
}

impl Debug for NamedProps<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for prop in self.props.decode() {
            match prop {
                Ok((key, value)) => map.entry(&self.names.name(key).unwrap_or_default(), &value),
                Err(err) => map.entry(&"error", &err),
            };
        }
        map.finish()
    }
}

struct Hex<'a>(&'a [u8]);

impl Debug for Hex<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

#[derive(Clone, Copy)]
enum DynamicKey {
    Unresolved,
    Resolved(Option<KeyId>),
}

fn key_count(columns: usize) -> u32 {
    u32::try_from(columns).expect("fewer than 2^32 columns")
}

fn output_key(
    cache: &mut Vec<DynamicKey>,
    layer: &PlannedLayer,
    keys: (&KeyInterner, &KeyInterner),
    key: KeyId,
) -> Option<KeyId> {
    let pos = key.index();
    if let Some(&copied) = layer.copy.get(pos) {
        return copied;
    }
    let slot = pos - layer.copy.len();
    if slot >= cache.len() {
        cache.resize(slot + 1, DynamicKey::Unresolved);
    }
    if let DynamicKey::Resolved(resolved) = cache[slot] {
        return resolved;
    }
    let (table, output) = keys;
    let resolved = table.name(key).and_then(|name| match &layer.attributes {
        AttributesDef::All => Some(output.intern(&name)),
        AttributesDef::None | AttributesDef::Computed { .. } => None,
        AttributesDef::Columns(selected) => selected.contains(&name).then(|| output.get(&name)).flatten(),
    });
    cache[slot] = DynamicKey::Resolved(resolved);
    resolved
}

fn learn_all(names: &mut (ExprKeys, u32), table: &KeyInterner) {
    let (names, known) = names;
    while let Some(name) = table.name(KeyId::new(*known)) {
        names.insert(&name, KeyId::new(*known));
        *known += 1;
    }
}

/// Borrows `value`, whether it is a plan literal or a source's property.
fn borrowed<S: AsRef<str>>(value: &Prop<S>) -> Prop<Cow<'_, str>> {
    match value {
        Prop::Bool(v) => Prop::Bool(*v),
        Prop::I64(v) => Prop::I64(*v),
        Prop::F32(v) => Prop::F32(*v),
        Prop::F64(v) => Prop::F64(*v),
        Prop::Str(v) => Prop::Str(Cow::Borrowed(v.as_ref())),
    }
}

/// Makes the table's keys interned since the last feature known to expressions.
fn learn_keys(names: &mut (ExprKeys, u32), props: &[(KeyId, Prop<SourceStr>)], table: &KeyInterner) {
    let (names, known) = names;
    for (key, _) in props {
        while *known <= key.get() {
            let id = KeyId::new(*known);
            if let Some(name) = table.name(id) {
                names.insert(&name, id);
            }
            *known += 1;
        }
    }
}
