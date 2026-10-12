//! CEL expressions over feature properties, compiled once and evaluated per feature.
//!
//! Bare identifiers are property names, and `feature['k']` reads keys that are not identifiers. A
//! missing property is `null`. Evaluation reads a [`FeatureView`] lazily: nothing is copied into a map.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fmt::Debug;
use std::ops::RangeInclusive;
use std::sync::{Arc, LazyLock};

use cel::common::ast::{CallExpr, EntryExpr, Expr, IdedExpr, LiteralValue, operators};
use cel::common::traits::{Container, Indexer};
use cel::common::types::{
    CelBool, CelDouble, CelInt, CelNull, CelString, CelUInt, Kind, MAP_TYPE, Type,
};
use cel::common::value::{CowVal, Val};
use cel::context::VariableResolver;
use cel::objects::ValueType;
use cel::{Context, Env, ExecutionError, FunctionContext, Value, extensions};
use martin_core::generate::props::{KeyId, Prop};
use mlt_core::PropValueRef;

const FEATURE: &str = "feature";
const COALESCE: &str = "coalesce";

static NULL: CelNull = CelNull;

type Function = Box<
    dyn for<'c, 'v> Fn(&mut FunctionContext<'c, 'v>) -> Result<CowVal<'c, 'v>, ExecutionError>
        + Send
        + Sync,
>;

static SHARED: LazyLock<Shared> = LazyLock::new(Shared::new);

struct Shared {
    env: Arc<Env>,
    root: Context<'static, 'static>,
}

impl Shared {
    fn new() -> Self {
        let mut env = Env::stdlib();
        env.add_extension(extensions::strings)
            .expect("the strings extension registers on the standard library");
        let env = Arc::new(env);
        let mut root = Context::with_env(Arc::clone(&env));
        root.add_function(COALESCE, Box::new(coalesce) as Function)
            .expect("the standard library has no coalesce");
        Self { env, root }
    }

    fn is_function(&self, name: &str) -> bool {
        name == COALESCE
            || Context::with_env(Arc::clone(&self.env))
                .add_function(name, Box::new(coalesce) as Function)
                .is_err()
    }

    fn is_type(&self, name: &str) -> bool {
        self.env.types().find_type(name).is_some()
    }
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "signature fixed by cel's function registry"
)]
fn coalesce<'c, 'v>(ftx: &mut FunctionContext<'c, 'v>) -> Result<CowVal<'c, 'v>, ExecutionError> {
    let args = std::mem::take(&mut ftx.args);
    Ok(ftx
        .this
        .take()
        .into_iter()
        .chain(args)
        .find(|v| v.get_type().kind() != Kind::NullType)
        .unwrap_or(CowVal::Borrowed(&NULL)))
}

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExprError {
    #[error("cannot parse `{expr}`: {message}")]
    Parse { expr: String, message: String },

    #[error("`{expr}` calls unknown function `{name}`")]
    UnknownFunction { expr: String, name: String },

    #[error("`{expr}` references unknown property `{name}`")]
    UnknownProperty { expr: String, name: String },
}

#[derive(Debug)]
pub struct CompiledExpr {
    expr: IdedExpr,
    columns: BTreeSet<String>,
    needs_all_keys: bool,
}

impl CompiledExpr {
    /// `columns` are the known property names. With `dynamic_props`, any other identifier is a property
    /// too, else it is an error.
    pub fn compile<S: AsRef<str>>(
        source: &str,
        columns: &[S],
        dynamic_props: bool,
    ) -> Result<Self, ExprError> {
        let shared = &*SHARED;
        let mut expr = shared
            .env
            .parser()
            .parse(source)
            .map_err(|e| ExprError::Parse {
                expr: source.to_owned(),
                message: e.to_string(),
            })?;
        let mut analyzer = Analyzer {
            shared,
            known: columns.iter().map(AsRef::as_ref).collect(),
            dynamic_props,
            bound: Vec::new(),
            columns: BTreeSet::new(),
            needs_all_keys: false,
        };
        analyzer.walk(&mut expr).map_err(|e| e.into_error(source))?;
        Ok(Self {
            expr,
            columns: analyzer.columns,
            needs_all_keys: analyzer.needs_all_keys,
        })
    }

    /// The property names this expression reads.
    pub fn columns(&self) -> impl Iterator<Item = &str> {
        self.columns.iter().map(String::as_str)
    }

    /// Whether the expression reads keys only known at evaluation time, e.g. `feature[k]`.
    #[must_use]
    pub fn needs_all_keys(&self) -> bool {
        self.needs_all_keys
    }

    /// The property this expression only reads, unchanged: a bare identifier or `feature['k']`.
    fn as_property(&self) -> Option<&str> {
        let name = if let Expr::Ident(name) = &self.expr.expr {
            name.as_str()
        } else if let Expr::Call(call) = &self.expr.expr
            && call.func_name == operators::INDEX
            && let [feature, key] = call.args.as_slice()
            && matches!(&feature.expr, Expr::Ident(name) if name == FEATURE)
            && let Expr::Literal(LiteralValue::String(key)) = &key.expr
        {
            key.inner()
        } else {
            return None;
        };
        self.columns.contains(name).then_some(name)
    }

    pub fn eval<'v, S>(&self, feature: &'v FeatureView<'_, S>) -> Result<Value, ExecutionError>
    where
        S: AsRef<str> + Debug + Send + Sync,
    {
        let root: &Context<'_, 'v> = &SHARED.root;
        let mut scope = root.new_inner_scope();
        scope.set_variable_resolver(feature);
        Value::resolve(&self.expr, &scope)
    }
}

/// What an expression is for, and how its result becomes that. A role value carries what is known when
/// the layer is planned, so evaluating needs nothing more.
pub trait Role {
    type Out;

    /// `None` is `null`: the filter, zoom, id or property is left out.
    fn coerce(&self, value: Value) -> Result<Option<Self::Out>, ExecutionError>;
}

#[derive(Clone, Copy, Debug)]
pub struct Filter;

#[derive(Clone, Copy, Debug)]
pub struct Zoom {
    lo: u8,
    hi: u8,
}

impl Zoom {
    #[must_use]
    pub fn new(range: &RangeInclusive<u8>) -> Self {
        Self {
            lo: *range.start(),
            hi: *range.end(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Id;

#[derive(Clone, Copy, Debug)]
pub struct Attr;

#[derive(Clone, Copy, Debug)]
pub struct SortKey {
    pub descending: bool,
}

impl Role for Filter {
    type Out = bool;

    fn coerce(&self, value: Value) -> Result<Option<bool>, ExecutionError> {
        to_filter(&value).map(Some)
    }
}

impl Role for Zoom {
    type Out = u8;

    fn coerce(&self, value: Value) -> Result<Option<u8>, ExecutionError> {
        to_zoom(&value, self.lo..=self.hi)
    }
}

impl Role for Id {
    type Out = u64;

    fn coerce(&self, value: Value) -> Result<Option<u64>, ExecutionError> {
        Ok(to_id(&value))
    }
}

impl Role for Attr {
    type Out = Prop<Cow<'static, str>>;

    fn coerce(&self, value: Value) -> Result<Option<Self::Out>, ExecutionError> {
        to_prop(value)
    }
}

impl Role for SortKey {
    type Out = Value;

    fn coerce(&self, value: Value) -> Result<Option<Value>, ExecutionError> {
        match value {
            Value::Null
            | Value::Bool(_)
            | Value::Int(_)
            | Value::UInt(_)
            | Value::Float(_)
            | Value::String(_) => Ok(Some(value)),
            other @ (Value::List(_)
            | Value::Map(_)
            | Value::Function(..)
            | Value::Bytes(_)
            | Value::Opaque(_)
            | Value::Struct(_)) => Err(mismatch(&other, "a scalar")),
        }
    }
}

/// An expression compiled for one [`Role`].
#[derive(Debug)]
pub struct TypedExpr<R> {
    expr: CompiledExpr,
    role: R,
}

impl<R: Role> TypedExpr<R> {
    pub fn compile<S: AsRef<str>>(
        source: &str,
        columns: &[S],
        dynamic_props: bool,
        role: R,
    ) -> Result<Self, ExprError> {
        let expr = CompiledExpr::compile(source, columns, dynamic_props)?;
        Ok(Self { expr, role })
    }

    pub fn columns(&self) -> impl Iterator<Item = &str> {
        self.expr.columns()
    }

    #[must_use]
    pub fn needs_all_keys(&self) -> bool {
        self.expr.needs_all_keys()
    }

    pub(crate) fn into_parts(self) -> (CompiledExpr, R) {
        (self.expr, self.role)
    }
}

impl TypedExpr<Attr> {
    /// The property this expression only reads, unchanged: a bare identifier or `feature['k']`.
    pub(crate) fn as_property(&self) -> Option<&str> {
        self.expr.as_property()
    }
}

/// Where a compiled expression sits in the plan, and the role it was compiled for.
#[derive(Clone, Copy, Debug)]
pub struct ExprRef<R> {
    pub(crate) index: usize,
    pub(crate) role: R,
}

impl<R: Role> ExprRef<R> {
    pub(crate) fn new(index: usize, role: R) -> Self {
        Self { index, role }
    }
}

enum WalkError {
    Function(String),
    Property(String),
}

impl WalkError {
    fn into_error(self, source: &str) -> ExprError {
        let expr = source.to_owned();
        match self {
            Self::Function(name) => ExprError::UnknownFunction { expr, name },
            Self::Property(name) => ExprError::UnknownProperty { expr, name },
        }
    }
}

struct Analyzer<'a> {
    shared: &'a Shared,
    known: BTreeSet<&'a str>,
    dynamic_props: bool,
    bound: Vec<String>,
    columns: BTreeSet<String>,
    needs_all_keys: bool,
}

impl Analyzer<'_> {
    fn is_bound(&self, name: &str) -> bool {
        self.bound.iter().any(|b| b == name)
    }

    fn is_feature(&self, expr: &IdedExpr) -> bool {
        matches!(&expr.expr, Expr::Ident(name) if name == FEATURE) && !self.is_bound(FEATURE)
    }

    fn key(&mut self, name: &str) -> Result<(), WalkError> {
        if self.dynamic_props || self.known.contains(name) {
            self.columns.insert(name.to_owned());
            Ok(())
        } else {
            Err(WalkError::Property(name.to_owned()))
        }
    }

    fn qualified_name(&self, expr: &IdedExpr) -> Option<String> {
        match &expr.expr {
            Expr::Ident(name) if !self.is_bound(name) => Some(name.clone()),
            Expr::Select(select) if !select.test => {
                let operand = self.qualified_name(&select.operand)?;
                Some(format!("{operand}.{}", select.field))
            }
            Expr::Unspecified
            | Expr::Call(_)
            | Expr::Comprehension(_)
            | Expr::Ident(_)
            | Expr::List(_)
            | Expr::Literal(_)
            | Expr::Map(_)
            | Expr::Select(_)
            | Expr::Struct(_) => None,
        }
    }

    fn walk(&mut self, expr: &mut IdedExpr) -> Result<(), WalkError> {
        let id = expr.id;
        match &mut expr.expr {
            Expr::Unspecified | Expr::Literal(_) => Ok(()),
            Expr::Ident(name) => {
                if self.is_bound(name) {
                    Ok(())
                } else if name == FEATURE {
                    self.needs_all_keys = true;
                    Ok(())
                } else if self.known.contains(name.as_str()) {
                    self.columns.insert(name.clone());
                    Ok(())
                } else if self.shared.is_type(name) {
                    Ok(())
                } else {
                    self.key(name)
                }
            }
            Expr::Select(select) => {
                if self.is_feature(&select.operand) {
                    let field = std::mem::take(&mut select.field);
                    self.key(&field)?;
                    let feature = std::mem::take(&mut *select.operand);
                    let key = IdedExpr {
                        id,
                        expr: Expr::Literal(LiteralValue::String(CelString::from(field))),
                    };
                    let (func_name, args) = if select.test {
                        (operators::IN, vec![key, feature])
                    } else {
                        (operators::INDEX, vec![feature, key])
                    };
                    expr.expr = Expr::Call(CallExpr {
                        func_name: func_name.to_owned(),
                        target: None,
                        args,
                    });
                    return Ok(());
                }
                if !select.test
                    && let Some(name) = self.qualified_name(&select.operand)
                    && self
                        .known
                        .contains(format!("{name}.{}", select.field).as_str())
                {
                    self.columns.insert(format!("{name}.{}", select.field));
                    return Ok(());
                }
                self.walk(&mut select.operand)
            }
            Expr::Call(call) => self.walk_call(call),
            Expr::Comprehension(comp) => {
                self.walk(&mut comp.iter_range)?;
                self.walk(&mut comp.accu_init)?;
                let depth = self.bound.len();
                self.bound.push(comp.iter_var.clone());
                self.bound.extend(comp.iter_var2.clone());
                self.bound.push(comp.accu_var.clone());
                let result = self
                    .walk(&mut comp.loop_cond)
                    .and_then(|()| self.walk(&mut comp.loop_step))
                    .and_then(|()| self.walk(&mut comp.result));
                self.bound.truncate(depth);
                result
            }
            Expr::List(list) => list.elements.iter_mut().try_for_each(|e| self.walk(e)),
            Expr::Map(map) => map
                .entries
                .iter_mut()
                .try_for_each(|entry| self.walk_entry(&mut entry.expr)),
            Expr::Struct(s) => s
                .entries
                .iter_mut()
                .try_for_each(|entry| self.walk_entry(&mut entry.expr)),
        }
    }

    fn walk_entry(&mut self, entry: &mut EntryExpr) -> Result<(), WalkError> {
        match entry {
            EntryExpr::StructField(field) => self.walk(&mut field.value),
            EntryExpr::MapEntry(entry) => {
                self.walk(&mut entry.key)?;
                self.walk(&mut entry.value)
            }
        }
    }

    fn walk_call(&mut self, call: &mut CallExpr) -> Result<(), WalkError> {
        let name = call.func_name.as_str();
        let is_index = name == operators::INDEX || name == operators::OPT_INDEX;
        if is_index && call.args.len() == 2 && self.is_feature(&call.args[0]) {
            if let Expr::Literal(LiteralValue::String(key)) = &call.args[1].expr {
                let key = key.inner().to_owned();
                return self.key(&key);
            }
            self.needs_all_keys = true;
            return self.walk(&mut call.args[1]);
        }
        if name == operators::IN
            && call.args.len() == 2
            && self.is_feature(&call.args[1])
            && let Expr::Literal(LiteralValue::String(key)) = &call.args[0].expr
        {
            let key = key.inner().to_owned();
            return self.key(&key);
        }
        let is_operator = name.starts_with(['_', '@', '!', '-']);
        let namespaced = call.target.as_deref().and_then(|t| self.qualified_name(t));
        let target_is_namespace = namespaced
            .is_some_and(|ns| self.shared.is_function(&format!("{ns}.{}", call.func_name)));
        if !is_operator && !target_is_namespace && !self.shared.is_function(&call.func_name) {
            return Err(WalkError::Function(call.func_name.clone()));
        }
        if !target_is_namespace && let Some(target) = &mut call.target {
            self.walk(target)?;
        }
        call.args.iter_mut().try_for_each(|arg| self.walk(arg))
    }
}

/// Property names of one table and their key ids. Built once per table, shared by every worker.
#[derive(Debug, Default, Clone)]
pub struct ExprKeys {
    keys: Vec<(Box<str>, KeyId)>,
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
        match self.keys.binary_search_by(|(k, _)| (**k).cmp(name)) {
            Ok(pos) => self.keys[pos].1 = key,
            Err(pos) => self.keys.insert(pos, (name.into(), key)),
        }
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<KeyId> {
        self.keys
            .binary_search_by(|(k, _)| (**k).cmp(name))
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
    pub fn bind<'a, S>(
        &'a mut self,
        keys: &'a ExprKeys,
        props: &'a [(KeyId, Prop<S>)],
    ) -> FeatureView<'a, S> {
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
        }
    }
}

#[derive(Debug)]
pub struct FeatureView<'a, S> {
    keys: &'a ExprKeys,
    slots: &'a [(u32, u32)],
    generation: u32,
    props: &'a [(KeyId, Prop<S>)],
}

impl<S> Clone for FeatureView<'_, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for FeatureView<'_, S> {}

impl<'a, S: AsRef<str>> FeatureView<'a, S> {
    fn prop(&self, key: KeyId) -> Option<PropValueRef<'a>> {
        let &(generation, pos) = self.slots.get(key.index())?;
        let props: &'a [(KeyId, Prop<S>)] = self.props;
        (generation == self.generation).then(|| props[pos as usize].1.as_ref())
    }

    fn lookup_val(&self, key: &dyn Val) -> Result<Option<PropValueRef<'a>>, ExecutionError> {
        let name =
            key.downcast_ref::<CelString>()
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
        let key = self.keys.get(variable)?;
        Some(prop_val(self.prop(key)))
    }
}

const TAG_NULL: u8 = 0;
const TAG_BOOL: u8 = 1;
const TAG_NEGATIVE: u8 = 2;
const TAG_NON_NEGATIVE: u8 = 3;
const TAG_DOUBLE: u8 = 4;
const TAG_STRING: u8 = 5;

fn mismatch(value: &Value, want: &str) -> ExecutionError {
    ExecutionError::UnexpectedType {
        got: value.type_of().to_string(),
        want: want.to_owned(),
    }
}

/// A filter result: `null` is `false`.
pub fn to_filter(value: &Value) -> Result<bool, ExecutionError> {
    match value {
        Value::Null => Ok(false),
        Value::Bool(v) => Ok(*v),
        other @ (Value::List(_)
        | Value::Map(_)
        | Value::Function(..)
        | Value::Int(_)
        | Value::UInt(_)
        | Value::Float(_)
        | Value::String(_)
        | Value::Bytes(_)
        | Value::Opaque(_)
        | Value::Struct(_)) => Err(other.error_expected_type(ValueType::Bool)),
    }
}

/// A zoom level, clamped into `range`.
pub fn to_zoom(value: &Value, range: RangeInclusive<u8>) -> Result<Option<u8>, ExecutionError> {
    let (lo, hi) = (*range.start(), *range.end());
    match *value {
        Value::Null => Ok(None),
        Value::Int(v) => Ok(Some(
            u8::try_from(v.clamp(i64::from(lo), i64::from(hi))).unwrap_or(hi),
        )),
        Value::UInt(v) => Ok(Some(
            u8::try_from(v.clamp(u64::from(lo), u64::from(hi))).unwrap_or(hi),
        )),
        ref other @ (Value::List(_)
        | Value::Map(_)
        | Value::Function(..)
        | Value::Float(_)
        | Value::String(_)
        | Value::Bytes(_)
        | Value::Bool(_)
        | Value::Opaque(_)
        | Value::Struct(_)) => Err(other.error_expected_type(ValueType::Int)),
    }
}

/// A property value; `null` leaves the property out.
pub fn to_prop(value: Value) -> Result<Option<Prop<Cow<'static, str>>>, ExecutionError> {
    Ok(Some(match value {
        Value::Null => return Ok(None),
        Value::Bool(v) => Prop::Bool(v),
        Value::Int(v) => Prop::I64(v),
        Value::UInt(v) => {
            i64::try_from(v).map_or_else(|_| Prop::Str(v.to_string().into()), Prop::I64)
        }
        Value::Float(v) => Prop::F64(v),
        Value::String(v) => Prop::Str(Arc::unwrap_or_clone(v).into()),
        other @ (Value::List(_)
        | Value::Map(_)
        | Value::Function(..)
        | Value::Bytes(_)
        | Value::Opaque(_)
        | Value::Struct(_)) => return Err(mismatch(&other, "a scalar")),
    }))
}

/// A feature id: only a non-negative integer is one.
#[must_use]
pub fn to_id(value: &Value) -> Option<u64> {
    match *value {
        Value::Int(v) => u64::try_from(v).ok(),
        Value::UInt(v) => Some(v),
        Value::Null
        | Value::Bool(_)
        | Value::Float(_)
        | Value::String(_)
        | Value::Bytes(_)
        | Value::List(_)
        | Value::Map(_)
        | Value::Function(..)
        | Value::Opaque(_)
        | Value::Struct(_) => None,
    }
}

/// Appends a key whose byte order is this value's order: by type (null, bool, int, double,
/// string), then by value. `descending` reverses it.
pub fn encode_sort(
    value: &Value,
    descending: bool,
    out: &mut Vec<u8>,
) -> Result<(), ExecutionError> {
    let start = out.len();
    match value {
        Value::Null => out.push(TAG_NULL),
        Value::Bool(v) => out.extend([TAG_BOOL, u8::from(*v)]),
        Value::Int(v) if *v < 0 => {
            out.push(TAG_NEGATIVE);
            out.extend((u64::from_ne_bytes(v.to_ne_bytes()) ^ (1 << 63)).to_be_bytes());
        }
        Value::Int(v) => {
            out.push(TAG_NON_NEGATIVE);
            out.extend(v.to_be_bytes());
        }
        Value::UInt(v) => {
            out.push(TAG_NON_NEGATIVE);
            out.extend(v.to_be_bytes());
        }
        Value::Float(v) => {
            let bits = v.to_bits();
            let bits = if bits >> 63 == 1 {
                !bits
            } else {
                bits | (1 << 63)
            };
            out.push(TAG_DOUBLE);
            out.extend(bits.to_be_bytes());
        }
        Value::String(v) => {
            out.push(TAG_STRING);
            for &b in v.as_bytes() {
                if b == 0 {
                    out.extend([0, 0xFF]);
                } else {
                    out.push(b);
                }
            }
            out.extend([0, 0]);
        }
        other @ (Value::List(_)
        | Value::Map(_)
        | Value::Function(..)
        | Value::Bytes(_)
        | Value::Opaque(_)
        | Value::Struct(_)) => return Err(mismatch(other, "a scalar")),
    }
    if descending {
        for b in &mut out[start..] {
            *b = !*b;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

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
        let expr =
            CompiledExpr::compile("feature['name:en']", &["name", "name:en"], false).unwrap();
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
        let expr =
            CompiledExpr::compile("feature['name:en'] == null", &["name:en"], false).unwrap();
        let keys = ExprKeys::columns(&["name:en"]);
        let props: Vec<(KeyId, Prop)> = vec![];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), Value::Bool(true));
    }

    #[rstest]
    #[case::first_present("coalesce(feature['name:en'], name)", Value::from("Munich"))]
    #[case::skips_null("coalesce(feature['name:de'], name)", Value::from("München"))]
    #[case::all_null("coalesce(feature['name:de'], null)", Value::Null)]
    #[case::literal_fallback("coalesce(population, 0)", Value::Int(0))]
    fn coalesce_returns_the_first_non_null(#[case] source: &str, #[case] expected: Value) {
        let columns = ["name", "name:en", "name:de", "population"];
        let expr = CompiledExpr::compile(source, &columns, false).unwrap();
        let keys = ExprKeys::columns(&columns);
        let props = vec![
            (KeyId::new(0), Prop::Str("München".to_owned())),
            (KeyId::new(1), Prop::Str("Munich".to_owned())),
        ];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), expected);
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

    #[rstest]
    #[case::select("feature.name.upperAscii()", Value::from("BERLIN "))]
    #[case::strings_extension("name.replace('l', 'L').trim()", Value::from("BerLin"))]
    #[case::comparison("population >= 1000000 && name.startsWith('B')", Value::Bool(true))]
    #[case::f32_is_double("area > 1.0", Value::Bool(true))]
    #[case::conversion("int(population / 1000)", Value::Int(3645))]
    #[case::comprehension("[1, 2].exists(x, x == rank)", Value::Bool(true))]
    #[case::ternary("population > 1000000 ? 'city' : 'town'", Value::from("city"))]
    fn expressions_evaluate_over_the_view(#[case] source: &str, #[case] expected: Value) {
        let columns = ["name", "population", "area", "rank"];
        let expr = CompiledExpr::compile(source, &columns, false).unwrap();
        let keys = ExprKeys::columns(&columns);
        let props = vec![
            (KeyId::new(0), Prop::Str("Berlin ".to_owned())),
            (KeyId::new(1), Prop::I64(3_645_000)),
            (KeyId::new(2), Prop::F32(891.8)),
            (KeyId::new(3), Prop::I64(2)),
        ];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).unwrap(), expected);
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
    fn a_runtime_error_is_typed() {
        let expr = CompiledExpr::compile("name + 1", &["name"], false).unwrap();
        let keys = ExprKeys::columns(&["name"]);
        let props = vec![(KeyId::new(0), Prop::Str("Berlin".to_owned()))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert!(matches!(
            expr.eval(&view),
            Err(ExecutionError::UnsupportedBinaryOperator("add", _, _))
        ));
    }

    #[rstest]
    #[case::unknown_function(
        "shout(name)",
        ExprError::UnknownFunction { expr: "shout(name)".to_owned(), name: "shout".to_owned() }
    )]
    #[case::unknown_method(
        "name.shout()",
        ExprError::UnknownFunction { expr: "name.shout()".to_owned(), name: "shout".to_owned() }
    )]
    #[case::unknown_identifier(
        "nmae == 'x'",
        ExprError::UnknownProperty { expr: "nmae == 'x'".to_owned(), name: "nmae".to_owned() }
    )]
    #[case::unknown_feature_key(
        "feature['name:fr']",
        ExprError::UnknownProperty { expr: "feature['name:fr']".to_owned(), name: "name:fr".to_owned() }
    )]
    fn unknown_names_are_rejected_at_compile_time(
        #[case] source: &str,
        #[case] expected: ExprError,
    ) {
        assert_eq!(
            CompiledExpr::compile(source, &["name"], false).unwrap_err(),
            expected
        );
    }

    #[test]
    fn a_syntax_error_is_a_parse_error() {
        let error = CompiledExpr::compile("name ==", &["name"], false).unwrap_err();
        assert!(matches!(error, ExprError::Parse { .. }));
    }

    #[rstest]
    #[case::bare("name", false, &["name"], false)]
    #[case::index("feature['name:en']", false, &["name:en"], false)]
    #[case::select("feature.name", false, &["name"], false)]
    #[case::has("has(feature.ref)", false, &["ref"], false)]
    #[case::in_feature("'ref' in feature", false, &["ref"], false)]
    #[case::mixed("coalesce(feature['name:en'], name) + string(rank)", false, &["name", "name:en", "rank"], false)]
    #[case::comprehension_var("[1, 2].exists(x, x == rank)", false, &["rank"], false)]
    #[case::type_name("type(rank) == int", false, &["rank"], false)]
    #[case::dynamic_index("feature[name]", false, &["name"], true)]
    #[case::bare_feature("size(feature) > 0", false, &[], true)]
    #[case::dynamic_identifier("surface == 'paved'", true, &["surface"], false)]
    fn references_are_collected(
        #[case] source: &str,
        #[case] dynamic_props: bool,
        #[case] columns: &[&str],
        #[case] needs_all_keys: bool,
    ) {
        let expr =
            CompiledExpr::compile(source, &["name", "name:en", "ref", "rank"], dynamic_props)
                .unwrap();
        assert_eq!(expr.columns().collect::<Vec<_>>(), columns);
        assert_eq!(expr.needs_all_keys(), needs_all_keys);
    }

    #[rstest]
    #[case::null(Value::Null, Ok(false))]
    #[case::t(Value::Bool(true), Ok(true))]
    #[case::f(Value::Bool(false), Ok(false))]
    #[case::int(Value::Int(1), Err(ExecutionError::UnexpectedType { got: "int".to_owned(), want: "bool".to_owned() }))]
    #[case::string(Value::from("yes"), Err(ExecutionError::UnexpectedType { got: "string".to_owned(), want: "bool".to_owned() }))]
    fn filter_coercion(#[case] value: Value, #[case] expected: Result<bool, ExecutionError>) {
        assert_eq!(to_filter(&value), expected);
    }

    #[rstest]
    #[case::null(Value::Null, Ok(None))]
    #[case::inside(Value::Int(7), Ok(Some(7)))]
    #[case::below(Value::Int(-3), Ok(Some(2)))]
    #[case::above(Value::Int(99), Ok(Some(14)))]
    #[case::uint(Value::UInt(u64::MAX), Ok(Some(14)))]
    #[case::double(Value::Float(5.0), Err(ExecutionError::UnexpectedType { got: "float".to_owned(), want: "int".to_owned() }))]
    fn zoom_coercion(#[case] value: Value, #[case] expected: Result<Option<u8>, ExecutionError>) {
        assert_eq!(to_zoom(&value, 2..=14), expected);
    }

    #[rstest]
    #[case::null(Value::Null, Ok(None))]
    #[case::string(Value::from("a"), Ok(Some(Prop::Str("a".into()))))]
    #[case::int(Value::Int(-4), Ok(Some(Prop::I64(-4))))]
    #[case::small_uint(Value::UInt(4), Ok(Some(Prop::I64(4))))]
    #[case::big_uint(Value::UInt(u64::MAX), Ok(Some(Prop::Str("18446744073709551615".into()))))]
    #[case::double(Value::Float(1.5), Ok(Some(Prop::F64(1.5))))]
    #[case::bool(Value::Bool(true), Ok(Some(Prop::Bool(true))))]
    #[case::list(Value::from(vec![1]), Err(ExecutionError::UnexpectedType { got: "list".to_owned(), want: "a scalar".to_owned() }))]
    #[case::map(Value::from(std::collections::HashMap::from([("k", 1)])), Err(ExecutionError::UnexpectedType { got: "map".to_owned(), want: "a scalar".to_owned() }))]
    fn prop_coercion(
        #[case] value: Value,
        #[case] expected: Result<Option<Prop<Cow<'static, str>>>, ExecutionError>,
    ) {
        assert_eq!(to_prop(value), expected);
    }

    #[test]
    fn a_list_result_cannot_be_a_prop() {
        let expr = CompiledExpr::compile("[name]", &["name"], false).unwrap();
        let keys = ExprKeys::columns(&["name"]);
        let props: Vec<(KeyId, Prop)> = vec![];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(
            to_prop(expr.eval(&view).unwrap()),
            Err(ExecutionError::UnexpectedType {
                got: "list".to_owned(),
                want: "a scalar".to_owned()
            })
        );
    }

    #[rstest]
    #[case::int(Value::Int(42), Some(42))]
    #[case::zero(Value::Int(0), Some(0))]
    #[case::negative(Value::Int(-1), None)]
    #[case::uint(Value::UInt(u64::MAX), Some(u64::MAX))]
    #[case::double(Value::Float(3.0), None)]
    #[case::string(Value::from("42"), None)]
    #[case::null(Value::Null, None)]
    fn id_coercion(#[case] value: Value, #[case] expected: Option<u64>) {
        assert_eq!(to_id(&value), expected);
    }

    #[test]
    fn sort_keys_order_like_their_values() {
        let ascending = [
            Value::Null,
            Value::Bool(false),
            Value::Bool(true),
            Value::Int(i64::MIN),
            Value::Int(-256),
            Value::Int(-1),
            Value::Int(0),
            Value::UInt(1),
            Value::Int(255),
            Value::UInt(256),
            Value::Int(i64::MAX),
            Value::UInt(u64::MAX),
            Value::Float(f64::NEG_INFINITY),
            Value::Float(-1e10),
            Value::Float(-1.5),
            Value::Float(-0.0),
            Value::Float(0.0),
            Value::Float(1e-300),
            Value::Float(2.5),
            Value::Float(f64::INFINITY),
            Value::from(""),
            Value::from("a"),
            Value::from("a\0"),
            Value::from("a\0b"),
            Value::from("a\x01"),
            Value::from("ab"),
            Value::from("b"),
            Value::from("é"),
        ];
        let keys: Vec<Vec<u8>> = ascending
            .iter()
            .map(|v| {
                let mut key = Vec::new();
                encode_sort(v, false, &mut key).unwrap();
                key
            })
            .collect();
        for (pair, values) in keys.windows(2).zip(ascending.windows(2)) {
            assert!(pair[0] < pair[1], "{:?} < {:?}", values[0], values[1]);
        }
        let descending: Vec<Vec<u8>> = ascending
            .iter()
            .map(|v| {
                let mut key = vec![9];
                encode_sort(v, true, &mut key).unwrap();
                key
            })
            .collect();
        for (pair, values) in descending.windows(2).zip(ascending.windows(2)) {
            assert!(
                pair[0] > pair[1],
                "{:?} > {:?} descending",
                values[0],
                values[1]
            );
        }
        assert!(descending.iter().all(|key| key[0] == 9));
    }

    #[test]
    fn a_list_has_no_sort_key() {
        let mut key = Vec::new();
        assert_eq!(
            encode_sort(&Value::from(vec![1]), false, &mut key),
            Err(ExecutionError::UnexpectedType {
                got: "list".to_owned(),
                want: "a scalar".to_owned()
            })
        );
        assert_eq!(key, Vec::<u8>::new());
    }
}
