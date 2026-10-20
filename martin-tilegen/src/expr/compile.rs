use std::collections::BTreeSet;
use std::fmt::Debug;

use cel::common::ast::{CallExpr, EntryExpr, Expr, IdedExpr, LiteralValue, operators};
use cel::common::types::CelString;
use cel::{Context, ExecutionError, Value};

use super::env::{SHARED, Shared};
use super::{FEATURE, FeatureView};

const OPERATORS: [&str; 21] = [
    operators::CONDITIONAL,
    operators::LOGICAL_AND,
    operators::LOGICAL_OR,
    operators::LOGICAL_NOT,
    operators::SUBSTRACT,
    operators::ADD,
    operators::MULTIPLY,
    operators::DIVIDE,
    operators::MODULO,
    operators::EQUALS,
    operators::NOT_EQUALS,
    operators::GREATER_EQUALS,
    operators::LESS_EQUALS,
    operators::GREATER,
    operators::LESS,
    operators::NEGATE,
    operators::INDEX,
    operators::OPT_INDEX,
    operators::OPT_SELECT,
    operators::NOT_STRICTLY_FALSE,
    operators::IN,
];

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
    pub(super) expr: IdedExpr,
    columns: BTreeSet<String>,
    needs_all_keys: bool,
}

impl CompiledExpr {
    /// `columns` are the known property names. With `dynamic_props`, any other identifier is a property
    /// too, else it is an error.
    pub fn compile<S: AsRef<str>>(source: &str, columns: &[S], dynamic_props: bool) -> Result<Self, ExprError> {
        let shared = &*SHARED;
        let mut expr = shared.env.parser().parse(source).map_err(|e| ExprError::Parse {
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
    pub(crate) fn as_property(&self) -> Option<&str> {
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
                    && self.known.contains(format!("{name}.{}", select.field).as_str())
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
        let is_operator = OPERATORS.contains(&name);
        let namespaced = call.target.as_deref().and_then(|t| self.qualified_name(t));
        let target_is_namespace =
            namespaced.is_some_and(|ns| self.shared.is_function(&format!("{ns}.{}", call.func_name)));
        if !is_operator && !target_is_namespace && !self.shared.is_function(&call.func_name) {
            return Err(WalkError::Function(call.func_name.clone()));
        }
        if !target_is_namespace && let Some(target) = &mut call.target {
            self.walk(target)?;
        }
        call.args.iter_mut().try_for_each(|arg| self.walk(arg))
    }
}

#[cfg(test)]
mod tests {
    use crate::props::{KeyId, Prop};
    use rstest::rstest;

    use super::*;
    use crate::expr::{ExprKeys, PropSlots};

    #[rstest]
    #[case::select("feature.name.upperAscii()", Value::from("BERLIN "))]
    #[case::strings_extension("name.replace('l', 'L').trim()", Value::from("BerLin"))]
    #[case::comparison("population >= 1000000 && name.startsWith('B')", Value::Bool(true))]
    #[case::f32_is_double("area > 1.0", Value::Bool(true))]
    #[case::conversion("int(population / 1000)", Value::Int(3645))]
    #[case::comprehension("[1, 2].exists(x, x == rank)", Value::Bool(true))]
    #[case::ternary("population > 1000000 ? 'city' : 'town'", Value::from("city"))]
    #[case::operators("!(rank > 5) && -rank < 0 && rank % 2 == 0 || area / 2.0 < 0.0", Value::Bool(true))]
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
    fn unknown_names_are_rejected_at_compile_time(#[case] source: &str, #[case] expected: ExprError) {
        assert_eq!(CompiledExpr::compile(source, &["name"], false).unwrap_err(), expected);
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
        let expr = CompiledExpr::compile(source, &["name", "name:en", "ref", "rank"], dynamic_props).unwrap();
        assert_eq!(expr.columns().collect::<Vec<_>>(), columns);
        assert_eq!(expr.needs_all_keys(), needs_all_keys);
    }
}
