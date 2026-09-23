use std::collections::HashMap;
use std::path::Path;

use super::Config;
use crate::config::file::{ConfigFileError, ConfigFileResult};
use crate::config::primitives::env::Env;

/// Read config from a file
pub fn read_config(file_name: &Path, env: &impl Env) -> ConfigFileResult<Config> {
    let contents = std::fs::read_to_string(file_name)
        .map_err(|e| ConfigFileError::ConfigLoadError(e, file_name.into()))?;
    parse_config(&contents, &env.as_property_map(), file_name)
}

pub fn parse_config(
    contents: &str,
    properties: &HashMap<String, String>,
    file_name: &Path,
) -> ConfigFileResult<Config> {
    // `with_snippet: false` keeps saphyr's hardcoded `<input>` source name out of the
    // diagnostic -- `ConfigFileError::to_miette_report` re-attaches a snippet against
    // our own NamedSource carrying the real file path.
    let options = serde_saphyr::options! {
        with_snippet: false,
        property_syntax: serde_saphyr::options::PropertySyntax::BracedOrBare,
    }
    .with_properties(properties.clone());

    serde_saphyr::from_str_with_options::<Config>(contents, options)
        .map_err(|e| ConfigFileError::yaml_parse(e, contents.to_owned(), file_name))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};
    use std::path::Path;
    use std::time::Duration;

    use rstest::rstest;

    use super::*;
    #[cfg(feature = "sprites")]
    use crate::config::file::FileConfigEnum;
    use crate::config::file::{CachePolicy, Config, GlobalCacheConfig};
    #[cfg(feature = "postgres")]
    use crate::config::primitives::OptOneMany;
    use crate::config::test_helpers::{render_failure, render_failure_json};

    fn parse_yaml(yaml: &str) -> Config {
        parse_config(
            yaml,
            &HashMap::<String, String>::new(),
            Path::new("test.yaml"),
        )
        .unwrap()
    }

    fn props(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn parse_with_env(yaml: &str, env: &HashMap<String, String>) -> Config {
        parse_config(yaml, env, Path::new("test.yaml")).unwrap()
    }

    #[test]
    fn syntax_error_unbalanced_quote() {
        insta::assert_snapshot!(
            render_failure(indoc::indoc! {r#"
                srv:
                  listen_addresses: "0.0.0.0:3000
                  worker_processes: 4
            "#}),
            @r#"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × invalid indentation in multiline quoted scalar
           ╭─[config.yaml:3:3]
         2 │   listen_addresses: "0.0.0.0:3000
         3 │   worker_processes: 4
           ·   ┬
           ·   ╰── invalid indentation in multiline quoted scalar
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        "#
        );
    }

    #[test]
    fn unknown_enum_variant_in_on_invalid() {
        insta::assert_snapshot!(render_failure("on_invalid: maybe\n"), @"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × unknown variant `maybe`, expected one of warn, abort
           ╭─[config.yaml:1:13]
         1 │ on_invalid: maybe
           ·             ──┬──
           ·               ╰── unknown variant `maybe`, expected one of warn, abort
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        ");
    }

    #[test]
    fn substitution_undefined_variable() {
        insta::assert_snapshot!(render_failure("cache:\n  size_mb: ${UNDEFINED_VAR}\n"), @"
        martin::config::substitution (https://maplibre.org/martin/config-file/)

          × missing property `UNDEFINED_VAR`
           ╭─[config.yaml:2:12]
         1 │ cache:
         2 │   size_mb: ${UNDEFINED_VAR}
           ·            ────────┬───────
           ·                    ╰── missing property `UNDEFINED_VAR`
           ╰────
          help: Make sure every ${VAR} reference resolves to an environment variable,
                or supply a default with `${VAR:-fallback}`.
        ");
    }

    #[test]
    fn substitution_single_colon_default() {
        insta::assert_snapshot!(render_failure("base_path: ${BASE:fallback}\n"), @"
        martin::config::substitution (https://maplibre.org/martin/config-file/)

          × Invalid name: '${BASE:fallback}'
           ╭─[config.yaml:1:12]
         1 │ base_path: ${BASE:fallback}
           ·            ────────┬───────
           ·                    ╰── Invalid name: '${BASE:fallback}'
           ╰────
          help: Make sure every ${VAR} reference resolves to an environment variable,
                or supply a default with `${VAR:-fallback}`.
        ");
    }

    #[test]
    fn cors_unsupported_scalar_renders_as_json() {
        // Mirrors what the binary emits when `RUST_LOG_FORMAT=json` is set: a structured
        // JSON document instead of the graphical snippet, suitable for editor tooling and
        // log aggregators.
        let json = render_failure_json("cors: 42\n");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).unwrap_or_else(|e| panic!("not JSON: {e}\n{json}"));

        let message = parsed.get("message").and_then(|m| m.as_str()).unwrap_or("");
        assert!(
            message.contains("invalid type: integer `42`"),
            "unexpected message in JSON output: {message}"
        );
        assert_eq!(
            parsed.get("severity").and_then(|s| s.as_str()),
            Some("error")
        );
        assert_eq!(
            parsed.get("filename").and_then(|f| f.as_str()),
            Some("config.yaml")
        );
        let labels = parsed.get("labels").and_then(|l| l.as_array()).unwrap();
        assert_eq!(labels.len(), 1, "expected one label, got {labels:?}");
        let span = labels[0].get("span").unwrap();
        assert!(span.get("offset").is_some(), "label missing offset");
        assert!(span.get("length").is_some(), "label missing length");
    }

    #[test]
    fn substitution_failure_renders_as_json() {
        let json = render_failure_json("cache:\n  size_mb: ${UNDEFINED_VAR}\n");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).unwrap_or_else(|e| panic!("not JSON: {e}\n{json}"));

        let message = parsed.get("message").and_then(|m| m.as_str()).unwrap_or("");
        assert!(
            message.contains("missing property `UNDEFINED_VAR`"),
            "unexpected message in JSON output: {message}"
        );
        assert_eq!(
            parsed.get("filename").and_then(|f| f.as_str()),
            Some("config.yaml")
        );
    }

    #[test]
    fn deprecated_cache_keys_are_unrecognized() {
        use crate::config::file::CollectUnrecognizedKeys as _;

        let config = parse_yaml("cache_size_mb: 512\ntile_cache_size_mb: 256");
        assert_eq!(config.cache, GlobalCacheConfig::default());
        let unrecognized = config.get_unrecognized_keys();
        let unrecognized = unrecognized.iter().collect::<BTreeSet<_>>();
        insta::assert_debug_snapshot!(unrecognized, @r#"
        {
            "cache_size_mb",
            "tile_cache_size_mb",
        }
        "#);
    }

    #[cfg(all(feature = "postgres", feature = "pmtiles", feature = "mlt"))]
    #[test]
    fn undocumented_key_aliases_are_unrecognized() {
        use crate::config::file::CollectUnrecognizedKeys as _;

        let config = parse_yaml(indoc::indoc! {"
            convert_to_mlt:
              allow_fpf: true
            pmtiles:
              aws_profile: default
            postgres:
              auto_publish:
                from_schema: public
                tables:
                  id_format: '{table}'
                  id_column: gid
                functions:
                  from_schema: public
        "});
        let unrecognized = config.get_unrecognized_keys();
        let unrecognized = unrecognized.iter().collect::<BTreeSet<_>>();
        insta::assert_debug_snapshot!(unrecognized, @r#"
        {
            "convert_to_mlt.allow_fpf",
            "pmtiles.aws_profile",
            "postgres.auto_publish.from_schema",
            "postgres.auto_publish.functions.from_schema",
            "postgres.auto_publish.tables.id_column",
            "postgres.auto_publish.tables.id_format",
        }
        "#);
    }

    #[test]
    fn new_cache_format_works_directly() {
        let config =
            parse_yaml("cache:\n  size_mb: 512\n  tile_size_mb: 256\n  minzoom: 2\n  maxzoom: 10");
        assert_eq!(config.cache.size_mb, Some(512));
        assert_eq!(config.cache.tile_size_mb, Some(256));
    }

    #[test]
    fn cache_disable_global() {
        let config = parse_yaml("cache: disable");
        assert_eq!(config.cache, GlobalCacheConfig::disabled());
        assert_eq!(config.cache.size_mb, Some(0));
        assert_eq!(config.cache.tile_size_mb, Some(0));
    }

    #[test]
    fn cache_disable_global_propagates_to_unconfigured_source() {
        let config = parse_yaml("cache: disable");
        let global_policy = config.cache.policy();
        let unconfigured_source = CachePolicy::default();
        let merged = unconfigured_source.or(global_policy);
        for zoom in 0..=u8::MAX {
            assert!(!merged.zoom().contains(zoom));
        }
    }

    #[cfg(feature = "sprites")]
    #[test]
    fn cache_disable_sprites() {
        let config = parse_yaml("sprites:\n  cache: disable\n  paths: /tmp");
        let FileConfigEnum::Config(cfg) = &config.sprites else {
            panic!("expected sprites config");
        };
        assert_eq!(cfg.custom.cache.size_mb, Some(0));
    }

    #[test]
    fn cache_expiry_global_config() {
        let config = parse_yaml("cache:\n  size_mb: 512\n  expiry: 1h\n  idle_timeout: 15m");
        assert_eq!(config.cache.size_mb, Some(512));
        assert_eq!(config.cache.expiry, Some(Duration::from_hours(1)));
        assert_eq!(config.cache.idle_timeout, Some(Duration::from_mins(15)));
    }

    #[test]
    fn cache_expiry_tile_specific() {
        let config = parse_yaml(
            "cache:\n  expiry: 1h\n  idle_timeout: 15m\n  tile_expiry: 30m\n  tile_idle_timeout: 5m",
        );
        assert_eq!(config.cache.expiry, Some(Duration::from_hours(1)));
        assert_eq!(config.cache.tile_expiry, Some(Duration::from_mins(30)));
        assert_eq!(config.cache.tile_idle_timeout, Some(Duration::from_mins(5)));
    }

    #[test]
    fn cache_expiry_none_when_unset() {
        let config = parse_yaml("cache:\n  size_mb: 512");
        assert_eq!(config.cache.expiry, None);
        assert_eq!(config.cache.idle_timeout, None);
        assert_eq!(config.cache.tile_expiry, None);
        assert_eq!(config.cache.tile_idle_timeout, None);
    }

    #[cfg(feature = "sprites")]
    #[test]
    fn cache_expiry_sprites() {
        let config = parse_yaml(
            "sprites:\n  cache:\n    size_mb: 64\n    expiry: 2h\n    idle_timeout: 30m\n  paths: /tmp",
        );
        let FileConfigEnum::Config(cfg) = &config.sprites else {
            panic!("expected sprites config");
        };
        assert_eq!(cfg.custom.cache.size_mb, Some(64));
        assert_eq!(cfg.custom.cache.expiry, Some(Duration::from_hours(2)));
        assert_eq!(cfg.custom.cache.idle_timeout, Some(Duration::from_mins(30)));
    }

    #[rstest]
    #[case::braced("${BASE}", "/my/path")]
    #[case::bare("$BASE", "/my/path")]
    #[case::dash_default_var_present("${BASE:-fallback}", "/my/path")]
    #[case::dash_default_var_unset("${UNSET:-/fallback}", "/fallback")]
    #[case::prefix_and_suffix("prefix-${BASE}-suffix", "prefix-/my/path-suffix")]
    #[case::escape_double_dollar("$$BASE", "$BASE")]
    fn substitution_accepted_forms(#[case] input: &str, #[case] expected: &str) {
        let env = props(&[("BASE", "/my/path")]);
        let yaml = format!("base_path: {input}\n");
        let config = parse_with_env(&yaml, &env);
        assert_eq!(config.srv.base_path.as_deref(), Some(expected));
    }

    #[cfg(feature = "postgres")]
    #[rstest]
    #[case::dash_default("${UNSET:-fallback}", Some("fallback"))]
    #[case::dash_default_unset_only("${UNSET-fallback}", Some("fallback"))]
    // an alternate that expands to nothing leaves an empty plain scalar, i.e. YAML null
    #[case::plus_alternate_unset("${UNSET:+set}", None)]
    #[case::plus_alternate_set("${BASE:+set}", Some("set"))]
    fn substitution_shell_operators(#[case] input: &str, #[case] expected: Option<&str>) {
        let env = props(&[("BASE", "/my/path")]);
        let yaml = format!("postgres:\n  connection_string: {input}\n");
        let config = parse_with_env(&yaml, &env);
        let pg = match config.postgres {
            OptOneMany::One(pg) => pg,
            other @ (OptOneMany::NoVals | OptOneMany::Many(_)) => {
                panic!("expected exactly one postgres config, got: {other:?}")
            }
        };
        assert_eq!(pg.connection_string.as_deref(), expected);
    }

    #[rstest]
    #[case::unquoted("base_path: ${BASE}\n", "/my/path")]
    #[case::single_quoted("base_path: '${BASE}'\n", "${BASE}")]
    #[case::double_quoted("base_path: \"${BASE}\"\n", "${BASE}")]
    fn substitution_only_in_plain_scalars(#[case] yaml: &str, #[case] expected: &str) {
        let env = props(&[("BASE", "/my/path")]);
        let config = parse_with_env(yaml, &env);
        assert_eq!(config.srv.base_path.as_deref(), Some(expected));
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn substitution_ignores_dollar_tokens_in_comments() {
        let yaml = indoc::indoc! {r"
            # Database configuration. This can also be a list of PG configs.
            postgres:
              # Database connection string. You can use env vars too, for example:
              #   $DATABASE_URL
              #   ${DATABASE_URL:-postgresql://postgres@localhost/db}
              connection_string: 'postgres://postgres:postgres@db:5432/ehrenamtskarte'
        "};
        let config = parse_config(
            yaml,
            &HashMap::<String, String>::new(),
            Path::new("config.yaml"),
        )
        .expect("comments containing ${VAR} must not trigger substitution");
        let one = match config.postgres {
            OptOneMany::One(pg) => pg,
            other @ (OptOneMany::NoVals | OptOneMany::Many(_)) => {
                panic!("expected exactly one postgres config, got: {other:?}")
            }
        };
        assert_eq!(
            one.connection_string.as_deref(),
            Some("postgres://postgres:postgres@db:5432/ehrenamtskarte"),
        );
    }
}
