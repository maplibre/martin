# cql2-rs

[![CI](https://github.com/developmentseed/cql2-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/developmentseed/cql2-rs/actions/workflows/ci.yml)

Parse, validate, and convert [Common Query Language (CQL2)](https://www.ogc.org/standard/cql2/) text and JSON.

## Usage

### API

```toml
[dependencies]
cql2 = "*"
```

Then:

```rust
use cql2::Expr;

let expr: Expr = "landsat:scene_id = 'LC82030282019133LGN00'".parse().unwrap();
assert!(expr.is_valid());
println!("{}", expr.to_json().unwrap());
```

See [the documentation](https://docs.rs/cql2) for more.

## CLI

See [the cql2-cli README](./cli/README.md) for details.

## Responses

Responses may not match the input.

### cql2-text differences

- Identifiers in output are quoted only where the cql2-text grammar requires it
- The position of "NOT" keywords is standardized to be before the expression (ie "... NOT LIKE ..." will become "NOT ... LIKE ..."
- The negative operator on anything besides a literal number becomes "* -1"
- Parentheses are added only where grouping requires them, so the output re-parses to the same expression

## Development

Get [uv](https://docs.astral.sh/uv/getting-started/installation/) and [Rust](https://rustup.rs/).
Then:

```shell
git clone git@github.com:developmentseed/cql2-rs.git
cd cql2-rs
uv sync
scripts/test
```

To lint all files:

```shell
scripts/lint
```

To serve the docs locally:

```shell
uv run mkdocs serve  # http://127.0.0.1:8000/cql2-rs/
```

See [CONTRIBUTING.md](./CONTRIBUTING.md) for more information about contributing to this project.

## License

**cql2-rs** is licensed under the MIT license.
See [LICENSE](./LICENSE) for details.
