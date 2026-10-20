use std::sync::{Arc, LazyLock};

use cel::common::types::Kind;
use cel::common::value::CowVal;
use cel::{Context, Env, ExecutionError, FunctionContext, extensions};

use super::NULL;

const COALESCE: &str = "coalesce";

type Function =
    Box<dyn for<'c, 'v> Fn(&mut FunctionContext<'c, 'v>) -> Result<CowVal<'c, 'v>, ExecutionError> + Send + Sync>;

pub(super) static SHARED: LazyLock<Shared> = LazyLock::new(Shared::new);

pub(super) struct Shared {
    pub(super) env: Arc<Env>,
    pub(super) root: Context<'static, 'static>,
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

    pub(super) fn is_function(&self, name: &str) -> bool {
        name == COALESCE
            || Context::with_env(Arc::clone(&self.env))
                .add_function(name, Box::new(coalesce) as Function)
                .is_err()
    }

    pub(super) fn is_type(&self, name: &str) -> bool {
        self.env.types().find_type(name).is_some()
    }
}

#[expect(clippy::unnecessary_wraps, reason = "signature fixed by cel's function registry")]
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

#[cfg(test)]
mod tests {
    use crate::props::{KeyId, Prop};
    use cel::Value;
    use rstest::rstest;

    use crate::expr::{CompiledExpr, ExprKeys, PropSlots};

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
}
