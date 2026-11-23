use std::collections::HashMap;
use std::fmt::Debug;

use cel::ExecutionError;
use cel::common::traits::{Container, Indexer, Iterable, Iterator as CelIterator, Sizer};
use cel::common::types::{
    CelBool, CelDouble, CelInt, CelList, CelMap, CelMapKey, CelNull, CelString, CelUInt, MAP_TYPE, Type,
};
use cel::common::value::{CowVal, Val};
use cel::context::VariableResolver;
use mlt_core::PropValueRef;

use super::area::{AreaBase, Areas};
use super::{FEATURE, NULL};
use crate::plan::{PlannedRelationSet, RELATIONS};
use crate::props::{KeyId, Prop};
use crate::source::Membership;

/// Property names of one table and their key ids. Built once per table, shared by every worker.
#[derive(Debug, Default, Clone)]
pub struct ExprKeys {
    keys: Vec<(CelString<'static>, KeyId)>,
}

impl ExprKeys {
    /// Keys declared up front: the `n`th name has [`KeyId`] `n`.
    pub fn columns<S: AsRef<str>>(names: &[S]) -> Self {
        (0..)
            .zip(names)
            .map(|(id, name)| (name.as_ref(), KeyId::new(id)))
            .collect()
    }

    pub fn insert(&mut self, name: &str, key: KeyId) {
        match self.keys.binary_search_by(|(k, _)| k.inner().cmp(name)) {
            Ok(pos) => self.keys[pos].1 = key,
            Err(pos) => self.keys.insert(pos, (CelString::from(name.to_owned()), key)),
        }
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<KeyId> {
        self.keys
            .binary_search_by(|(k, _)| k.inner().cmp(name))
            .ok()
            .map(|pos| self.keys[pos].1)
    }
}

impl<'n> FromIterator<(&'n str, KeyId)> for ExprKeys {
    fn from_iter<I: IntoIterator<Item = (&'n str, KeyId)>>(iter: I) -> Self {
        let mut keys = Self::default();
        for (name, key) in iter {
            keys.insert(name, key);
        }
        keys
    }
}

/// Where each key's value sits in the current feature's props, by [`KeyId`]. One per worker, reused
/// from feature to feature.
#[derive(Debug, Default)]
pub struct PropSlots {
    slots: Vec<(u32, u32)>,
    generation: u32,
}

impl PropSlots {
    /// Indexes `props` once; the view then serves every expression of every layer for this feature.
    pub fn bind<'a, S>(&'a mut self, keys: &'a ExprKeys, props: &'a [(KeyId, Prop<S>)]) -> FeatureView<'a, S> {
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.slots.fill((0, 0));
            self.generation = 1;
        }
        for (pos, (key, _)) in props.iter().enumerate() {
            let idx = key.index();
            if idx >= self.slots.len() {
                self.slots.resize(idx + 1, (0, 0));
            }
            let pos = u32::try_from(pos).expect("fewer than 2^32 properties per feature");
            self.slots[idx] = (self.generation, pos);
        }
        FeatureView {
            keys,
            slots: &self.slots,
            generation: self.generation,
            props,
            areas: None,
            relations: None,
        }
    }
}

#[derive(Debug)]
pub struct FeatureView<'a, S> {
    keys: &'a ExprKeys,
    slots: &'a [(u32, u32)],
    generation: u32,
    props: &'a [(KeyId, Prop<S>)],
    /// The feature's whole geometry for `area()`, which fails without it.
    areas: Option<&'a Areas<'a>>,
    /// The relations the feature is a member of, when its table has relation sets.
    relations: Option<Relations<'a>>,
}

/// The relation sets of a table, and the relations of them that one feature is a member of.
#[derive(Clone, Copy, Debug)]
struct Relations<'a> {
    sets: &'a [PlannedRelationSet],
    members: &'a [Membership],
}

impl<'a> Relations<'a> {
    /// `relations`: each set by name, as the list of the feature's [`member`](Self::member) maps of it.
    fn to_val(self) -> CelMap<'a> {
        let mut sets: HashMap<CelMapKey<'a>, Box<dyn Val + 'a>> = HashMap::with_capacity(self.sets.len());
        for (index, set) in self.sets.iter().enumerate() {
            let members: Vec<Box<dyn Val + 'a>> = self
                .members
                .iter()
                .filter(|member| usize::from(member.set) == index)
                .map(|member| Box::new(Self::member(set, member)) as Box<dyn Val + 'a>)
                .collect();
            sets.insert(CelMapKey::from(set.def.name.as_str()), Box::new(CelList::from(members)));
        }
        CelMap::from(sets)
    }

    /// One relation of `set` as its `member` sees it: the set's tags, `null` when missing, its `id` and the `role`.
    fn member(set: &'a PlannedRelationSet, member: &'a Membership) -> CelMap<'a> {
        let mut map: HashMap<CelMapKey<'a>, Box<dyn Val + 'a>> = HashMap::with_capacity(set.def.tags.len() + 2);
        for (tag, value) in set.def.tags.iter().zip(&member.tags) {
            let value: Box<dyn Val + 'a> = match value {
                Some(value) => Box::new(CelString::from(&**value)),
                None => Box::new(CelNull),
            };
            map.insert(CelMapKey::from(tag.as_str()), value);
        }
        map.insert(CelMapKey::from("id"), Box::new(CelInt::from(member.id)));
        map.insert(CelMapKey::from("role"), Box::new(CelString::from(&*member.role)));
        CelMap::from(map)
    }
}

impl<S> Clone for FeatureView<'_, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for FeatureView<'_, S> {}

impl<'a, S> FeatureView<'a, S> {
    /// This view, with `areas` measuring the feature's whole geometry for `area()`.
    #[must_use]
    pub fn with_areas(self, areas: &'a Areas<'a>) -> Self {
        Self {
            areas: Some(areas),
            ..self
        }
    }

    /// This view, with `relations` reading the relations of `sets` the feature is a member of; a table without sets
    /// keeps `relations` a property.
    #[must_use]
    pub(crate) fn with_relations(self, sets: &'a [PlannedRelationSet], members: &'a [Membership]) -> Self {
        Self {
            relations: (!sets.is_empty()).then_some(Relations { sets, members }),
            ..self
        }
    }
}

impl<'a, S: AsRef<str>> FeatureView<'a, S> {
    pub(super) fn prop(&self, key: KeyId) -> Option<PropValueRef<'a>> {
        let &(generation, pos) = self.slots.get(key.index())?;
        let props: &'a [(KeyId, Prop<S>)] = self.props;
        (generation == self.generation).then(|| props[pos as usize].1.as_ref())
    }

    /// The key `name` names, if the table has one.
    pub(super) fn key(&self, name: &str) -> Option<KeyId> {
        self.keys.get(name)
    }

    fn lookup_val(&self, key: &dyn Val) -> Result<Option<PropValueRef<'a>>, ExecutionError> {
        let name = key
            .downcast_ref::<CelString>()
            .ok_or_else(|| ExecutionError::UnexpectedType {
                got: key.get_type().name().to_owned(),
                want: "string".to_owned(),
            })?;
        Ok(self.keys.get(name.inner()).and_then(|key| self.prop(key)))
    }
}

fn prop_val<'b>(prop: Option<PropValueRef<'_>>) -> CowVal<'b, '_> {
    match prop {
        None => CowVal::Borrowed(&NULL),
        Some(PropValueRef::Bool(v)) => CowVal::owned(CelBool::from(v)),
        Some(PropValueRef::I8(v)) => CowVal::owned(CelInt::from(i64::from(v))),
        Some(PropValueRef::I32(v)) => CowVal::owned(CelInt::from(i64::from(v))),
        Some(PropValueRef::I64(v)) => CowVal::owned(CelInt::from(v)),
        Some(PropValueRef::U8(v)) => CowVal::owned(CelUInt::from(u64::from(v))),
        Some(PropValueRef::U32(v)) => CowVal::owned(CelUInt::from(u64::from(v))),
        Some(PropValueRef::U64(v)) => CowVal::owned(CelUInt::from(v)),
        Some(PropValueRef::F32(v)) => CowVal::owned(CelDouble::from(f64::from(v))),
        Some(PropValueRef::F64(v)) => CowVal::owned(CelDouble::from(v)),
        Some(PropValueRef::Str(v)) => CowVal::owned(CelString::from(v)),
    }
}

impl<S: AsRef<str> + Debug + Send + Sync> Val for FeatureView<'_, S> {
    fn get_type(&self) -> &Type {
        &MAP_TYPE
    }

    fn cel_type() -> &'static Type {
        &MAP_TYPE
    }

    fn as_container(&self) -> Option<&dyn Container> {
        Some(self)
    }

    fn as_sizer(&self) -> Option<&dyn Sizer> {
        Some(self)
    }

    fn as_iterable<'b, 'w>(&'b self) -> Option<&'b (dyn Iterable + 'w)>
    where
        Self: 'w,
    {
        Some(self)
    }

    fn as_indexer<'b, 'w>(&'b self) -> Option<&'b (dyn Indexer + 'w)>
    where
        Self: 'w,
    {
        Some(self)
    }

    fn into_indexer<'w>(self: Box<Self>) -> Option<Box<dyn Indexer + 'w>>
    where
        Self: 'w,
    {
        Some(self)
    }

    fn clone_as_boxed<'w>(&self) -> Box<dyn Val + 'w>
    where
        Self: 'w,
    {
        Box::new(*self)
    }
}

struct PresentKeys<'a, S> {
    keys: std::slice::Iter<'a, (CelString<'static>, KeyId)>,
    view: FeatureView<'a, S>,
}

impl<'a: 'b, 'b, 'w, S: AsRef<str>> CelIterator<'b, 'w> for PresentKeys<'a, S> {
    fn next(&mut self) -> Option<&'b (dyn Val + 'w)> {
        let view = self.view;
        self.keys
            .find(|(_, key)| view.prop(*key).is_some())
            .map(|(name, _)| name as &dyn Val)
    }
}

impl<S: AsRef<str>> Iterable for FeatureView<'_, S> {
    fn iter<'b, 'w>(&'b self) -> Box<dyn CelIterator<'b, 'w> + 'b>
    where
        Self: 'w,
    {
        Box::new(PresentKeys {
            keys: self.keys.keys.iter(),
            view: *self,
        })
    }
}

impl<S: AsRef<str>> Sizer for FeatureView<'_, S> {
    fn size(&self) -> CelInt {
        let present = self.keys.keys.iter().filter(|(_, key)| self.prop(*key).is_some());
        CelInt::from(i64::try_from(present.count()).expect("fewer than 2^63 properties"))
    }
}

impl<S: AsRef<str>> Container for FeatureView<'_, S> {
    fn contains(&self, key: &dyn Val) -> Result<bool, ExecutionError> {
        Ok(self.lookup_val(key)?.is_some())
    }
}

impl<S: AsRef<str>> Indexer for FeatureView<'_, S> {
    fn get<'b, 'w>(&'b self, key: &dyn Val) -> Result<CowVal<'b, 'w>, ExecutionError>
    where
        Self: 'w,
    {
        Ok(prop_val(self.lookup_val(key)?))
    }

    fn steal<'w>(self: Box<Self>, key: &dyn Val) -> Result<Box<dyn Val + 'w>, ExecutionError>
    where
        Self: 'w,
    {
        self.get(key).map(CowVal::into_owned)
    }
}

impl<S: AsRef<str> + Debug + Send + Sync> VariableResolver for FeatureView<'_, S> {
    fn resolve<'b>(&'b self, variable: &str) -> Option<CowVal<'b, 'b>> {
        if variable == FEATURE {
            return Some(CowVal::Borrowed(self));
        }
        if let Some(base) = [AreaBase::SquareMeters, AreaBase::Z0Tiles]
            .into_iter()
            .find(|base| base.variable() == variable)
        {
            return self.areas.map(|areas| CowVal::owned(CelDouble::from(areas.get(base))));
        }
        if variable == RELATIONS
            && let Some(relations) = self.relations
        {
            return Some(CowVal::owned(relations.to_val()));
        }
        let key = self.keys.get(variable)?;
        Some(prop_val(self.prop(key)))
    }
}

#[cfg(test)]
mod tests {
    use cel::Value;
    use rstest::rstest;

    use super::*;
    use crate::expr::CompiledExpr;

    #[test]
    fn a_bare_name_reads_the_property() {
        let expr = CompiledExpr::compile("name", &["name", "population"], false).unwrap();
        let keys = ExprKeys::columns(&["name", "population"]);
        let props = vec![(KeyId::new(0), Prop::Str("Berlin".to_owned()))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), Value::from("Berlin"));
    }

    #[test]
    fn a_missing_column_is_null() {
        let expr = CompiledExpr::compile("name", &["name", "population"], false).unwrap();
        let keys = ExprKeys::columns(&["name", "population"]);
        let props = vec![(KeyId::new(1), Prop::<String>::I64(5))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), Value::Null);
    }

    #[test]
    fn feature_index_reads_keys_that_are_not_identifiers() {
        let expr = CompiledExpr::compile("feature['name:en']", &["name", "name:en"], false).unwrap();
        let keys = ExprKeys::columns(&["name", "name:en"]);
        let props = vec![
            (KeyId::new(0), Prop::Str("München".to_owned())),
            (KeyId::new(1), Prop::Str("Munich".to_owned())),
        ];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), Value::from("Munich"));
    }

    #[test]
    fn feature_index_of_a_missing_key_is_null() {
        let expr = CompiledExpr::compile("feature['name:en'] == null", &["name:en"], false).unwrap();
        let keys = ExprKeys::columns(&["name:en"]);
        let props: Vec<(KeyId, Prop)> = vec![];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), Value::Bool(true));
    }

    #[rstest]
    #[case::present("has(feature.name)", true)]
    #[case::absent("has(feature.ref)", false)]
    #[case::in_present("'name' in feature", true)]
    #[case::in_absent("'ref' in feature", false)]
    fn presence_tests_see_missing_keys(#[case] source: &str, #[case] expected: bool) {
        let expr = CompiledExpr::compile(source, &["name", "ref"], false).unwrap();
        let keys = ExprKeys::columns(&["name", "ref"]);
        let props = vec![(KeyId::new(0), Prop::Str("Berlin".to_owned()))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), Value::Bool(expected));
    }

    #[test]
    fn slots_are_reused_from_feature_to_feature() {
        let expr = CompiledExpr::compile("coalesce(name, ref)", &["name", "ref"], false).unwrap();
        let keys = ExprKeys::columns(&["name", "ref"]);
        let mut slots = PropSlots::default();
        let first = vec![(KeyId::new(0), Prop::Str("Berlin".to_owned()))];
        let view = slots.bind(&keys, &first);
        assert_eq!(expr.eval(&view).unwrap(), Value::from("Berlin"));
        let second = vec![(KeyId::new(1), Prop::Str("A 100".to_owned()))];
        let view = slots.bind(&keys, &second);
        assert_eq!(expr.eval(&view).unwrap(), Value::from("A 100"));
    }

    #[test]
    fn dynamic_props_resolve_through_inserted_keys() {
        let expr = CompiledExpr::compile("highway == 'primary'", &["name"], true).unwrap();
        assert_eq!(expr.columns().collect::<Vec<_>>(), ["highway"]);
        let mut keys = ExprKeys::columns(&["name"]);
        keys.insert("highway", KeyId::new(7));
        let props = vec![(KeyId::new(7), Prop::Str("primary".to_owned()))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), Value::Bool(true));
    }

    #[test]
    fn size_of_feature_counts_the_present_properties() {
        let expr = CompiledExpr::compile("size(feature)", &["name", "ref", "rank"], false).unwrap();
        let keys = ExprKeys::columns(&["name", "ref", "rank"]);
        let props = vec![
            (KeyId::new(0), Prop::Str("Berlin".to_owned())),
            (KeyId::new(2), Prop::I64(2)),
        ];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), Value::Int(2));
    }

    #[test]
    fn iterating_feature_yields_the_present_property_names() {
        let expr = CompiledExpr::compile("feature.map(k, k)", &["ref", "name", "rank"], false).unwrap();
        let keys = ExprKeys::columns(&["ref", "name", "rank"]);
        let props = vec![
            (KeyId::new(2), Prop::I64(2)),
            (KeyId::new(1), Prop::Str("Berlin".to_owned())),
        ];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), Value::from(vec!["name", "rank"]));
    }
}
