use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

use tracing::warn;

use super::Config;
use crate::config::file::{ConfigFileError, ConfigFileResult};
use crate::config::primitives::env::Env;

/// Read config from a file
pub fn read_config(file_name: &Path, env: &impl Env) -> ConfigFileResult<Config> {
    let contents = std::fs::read_to_string(file_name)
        .map_err(|e| ConfigFileError::ConfigLoadError(e, file_name.into()))?;
    #[cfg(feature = "postgres")]
    warn_unused_pg_env_vars(env, &contents);
    parse_config(&contents, &env.as_property_map(), file_name)
}

#[cfg(feature = "postgres")]
fn warn_unused_pg_env_vars(env: &impl Env, contents: &str) {
    let set_vars: Vec<&str> = [
        "DATABASE_URL",
        "DEFAULT_SRID",
        "PGSSLCERT",
        "PGSSLKEY",
        "PGSSLROOTCERT",
    ]
    .into_iter()
    .filter(|v| env.var_os(v).is_some())
    .collect();
    if set_vars.is_empty() {
        return;
    }
    let Ok(value) = serde_saphyr::from_str::<serde_json::Value>(contents) else {
        return;
    };
    for v in set_vars {
        if !json_value_references_var(&value, v) {
            warn!(
                "Environment variable {v} is set, but will be ignored because a configuration file was loaded. Any environment variables can be used inside the config yaml file."
            );
        }
    }
}

#[cfg(feature = "postgres")]
fn json_value_references_var(value: &serde_json::Value, name: &str) -> bool {
    use serde_json::Value::{Array, Bool, Null, Number, Object, String};
    match value {
        String(s) => string_references_var(s, name),
        Array(items) => items.iter().any(|v| json_value_references_var(v, name)),
        Object(map) => map
            .iter()
            .any(|(k, v)| string_references_var(k, name) || json_value_references_var(v, name)),
        Null | Bool(_) | Number(_) => false,
    }
}

/// Whether `haystack` contains a saphyr-style substitution token referencing `name`:
/// `${name}`, `${name<op>...}` for `<op>` in `:-/-/:+/+/:?/?`, or bare `$name` not
/// followed by an identifier-continuation char. `$$` is treated as an escape, not a `$`.
#[cfg(feature = "postgres")]
fn string_references_var(haystack: &str, name: &str) -> bool {
    let mut rest = haystack;
    while let Some((_, after)) = rest.split_once('$') {
        if let Some(escaped) = after.strip_prefix('$') {
            rest = escaped;
            continue;
        }
        let (body, in_braces) = after
            .strip_prefix('{')
            .map_or((after, false), |b| (b, true));
        if let Some(tail) = body.strip_prefix(name) {
            let next = tail.bytes().next();
            let matched = if in_braces {
                matches!(next, Some(b'}' | b':' | b'-' | b'+' | b'?'))
            } else {
                next.is_none_or(|c| !(c.is_ascii_alphanumeric() || c == b'_'))
            };
            if matched {
                return true;
            }
        }
        rest = after;
    }
    false
}

pub fn parse_config(
    contents: &str,
    properties: &HashMap<String, String>,
    file_name: &Path,
) -> ConfigFileResult<Config> {
    let contents = rewrite_legacy_substitution_syntax(contents);
    let migrated = if needs_deprecated_migration(&contents) {
        match serde_saphyr::from_str::<serde_json::Value>(&contents) {
            Ok(mut value) => {
                migrate_deprecated_config(&mut value);
                serde_saphyr::to_string(&value).unwrap_or_else(|_| contents.to_string())
            }
            Err(_) => contents.to_string(),
        }
    } else {
        contents.to_string()
    };

    // `with_snippet: false` keeps saphyr's hardcoded `<input>` source name out of the
    // diagnostic -- `ConfigFileError::to_miette_report` re-attaches a snippet against
    // our own NamedSource carrying the real file path.
    let options = serde_saphyr::options! {
        with_snippet: false,
        property_syntax: serde_saphyr::options::PropertySyntax::BracedOrBare,
    }
    .with_properties(properties.clone());

    serde_saphyr::from_str_with_options::<Config>(&migrated, options)
        .map_err(|e| ConfigFileError::yaml_parse(e, migrated, file_name))
}

/// Rewrites the legacy single-colon default `${VAR:default}` to serde-saphyr's `${VAR:-default}`,
/// warning once when anything changes.
///
/// Martin <= 1.10 used the `subst` crate's single-colon syntax; serde-saphyr rejects it outright,
/// so without this those configs fail to load. Substitution inside quoted scalars and `subst`'s
/// backslash escaping are not restored -- both are unrelated to the single-colon default.
fn rewrite_legacy_substitution_syntax(contents: &str) -> Cow<'_, str> {
    if !contents.contains("${") {
        return Cow::Borrowed(contents);
    }

    // One segment per reference body, so nested `${a:x${b:y}}` splits into its own body.
    let mut segments = contents.split("${");
    let first = segments.next().unwrap_or("");
    let mut out = String::with_capacity(contents.len() + 8);
    out.push_str(first);

    let mut prev = first;
    let mut rewrites = 0usize;
    for body in segments {
        out.push_str("${");
        // A `${` after an odd run of `$` is escaped (`$$` -> literal `$`), not a reference.
        let escaped = prev.bytes().rev().take_while(|&b| b == b'$').count() % 2 == 1;
        match rewrite_reference_body(body).filter(|_| !escaped) {
            Some(rewritten) => {
                out.push_str(&rewritten);
                rewrites += 1;
            }
            None => out.push_str(body),
        }
        prev = body;
    }

    if rewrites == 0 {
        return Cow::Borrowed(contents);
    }

    warn!(
        deprecated_tokens = rewrites,
        "Deprecated `${{VAR:default}}` substitution syntax in config; use `${{VAR:-default}}`. Support for the single-colon form will be removed in a future release."
    );
    Cow::Owned(out)
}

/// Rewrites the operator colon (the first `:` before the closing `}`) of one `${...}` body from
/// `:default` to `:-default`, returning `None` when it is already an operator or absent.
fn rewrite_reference_body(body: &str) -> Option<String> {
    let end = body.find('}').unwrap_or(body.len());
    let (name, default) = body.get(..end)?.split_once(':')?;
    if default.starts_with(['-', '+', '?']) {
        return None;
    }
    let rest = body.get(end..).unwrap_or_default();
    Some(format!("{name}:-{default}{rest}"))
}

/// Cheap pre-check: does the YAML mention any deprecated cache key?
///
/// False positives are harmless (the fast path is identical to the slow path's no-op
/// migration), so a substring search is sufficient.
fn needs_deprecated_migration(yaml: &str) -> bool {
    yaml.contains("cache_size_mb")
        || yaml.contains("tile_cache_size_mb")
        || yaml.contains("directory_cache_size_mb")
}

/// Migrates deprecated cache configuration keys in raw YAML before deserialization.
///
/// This runs on the `serde_json::Value` directly, so the `Config` struct
/// never needs to know about deprecated field names.
fn migrate_deprecated_config(value: &mut serde_json::Value) {
    let Some(root) = value.as_object_mut() else {
        return;
    };

    migrate_json_key(root, "cache_size_mb", &["cache", "size_mb"]);
    migrate_json_key(root, "tile_cache_size_mb", &["cache", "tile_size_mb"]);

    for section in ["sprites", "fonts"] {
        if let Some(mapping) = root.get_mut(section).and_then(|v| v.as_object_mut()) {
            migrate_json_key(mapping, "cache_size_mb", &["cache", "size_mb"]);
        }
    }

    if let Some(mapping) = root.get_mut("pmtiles").and_then(|v| v.as_object_mut()) {
        migrate_json_key(
            mapping,
            "directory_cache_size_mb",
            &["directory_cache", "size_mb"],
        );
    }
}

/// Moves a deprecated key in a JSON map to a new nested location.
///
/// `new_path` is a slice of keys describing the nested destination,
/// e.g. `&["cache", "size_mb"]` means `cache.size_mb`.
///
/// If the new key already exists, the old value is dropped with a warning.
/// If only the old key exists, it is moved to the new location.
fn migrate_json_key(
    mapping: &mut serde_json::Map<String, serde_json::Value>,
    old_key: &str,
    new_path: &[&str],
) {
    debug_assert!(!new_path.is_empty(), "new_path must not be empty");

    let Some(old_value) = mapping.remove(old_key) else {
        return;
    };

    let new_key_display = new_path.join(".");

    let [parents @ .., leaf] = new_path else {
        return;
    };
    let mut current = &mut *mapping;
    for &segment in parents {
        if !current.contains_key(segment) {
            current.insert(
                segment.to_string(),
                serde_json::Value::Object(serde_json::Map::default()),
            );
        }
        let Some(nested) = current.get_mut(segment).and_then(|v| v.as_object_mut()) else {
            warn!(
                "deprecated config: `{old_key}` is ignored because `{segment}` is already set. \
                 Please remove `{old_key}` from your configuration"
            );
            return;
        };
        current = nested;
    }

    if current.contains_key(*leaf) {
        warn!(
            "deprecated config: `{old_key}` is ignored in favor of `{new_key_display}`. \
             Please remove `{old_key}` from your configuration"
        );
    } else {
        current.insert((*leaf).to_string(), old_value);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::time::Duration;

    use rstest::rstest;

    use super::*;
    #[cfg(any(feature = "sprites", feature = "fonts"))]
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
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    fn parse_with_env(yaml: &str, env: &HashMap<String, String>) -> Config {
        parse_config(yaml, env, Path::new("test.yaml")).unwrap()
    }

    // ----- `parse_config` pipeline diagnostics: failures that don't belong to a single
    // ----- field's deserializer (raw YAML syntax, ${VAR} substitution, derive-`Deserialize`
    // ----- enums) live here next to the function under test.

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

          × unknown variant `maybe`, expected one of continue, ignore, warn, warning,
          │ warnings, abort
           ╭─[config.yaml:1:13]
         1 │ on_invalid: maybe
           ·             ──┬──
           ·               ╰── unknown variant `maybe`, expected one of continue, ignore, warn, warning, warnings, abort
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        ");
    }

    #[test]
    fn substitution_undefined_variable() {
        insta::assert_snapshot!(render_failure("cache_size_mb: ${UNDEFINED_VAR}\n"), @"
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
        let json = render_failure_json("cache_size_mb: ${UNDEFINED_VAR}\n");
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
    fn cache_migrates_old_to_new_cache_config_key() {
        let config = parse_yaml("cache_size_mb: 512");
        assert_eq!(config.cache.size_mb, Some(512));
    }

    #[test]
    fn migrate_tile_cache_size_mb_to_cache_tile_size_mb() {
        let config = parse_yaml("tile_cache_size_mb: 256");
        assert_eq!(config.cache.tile_size_mb, Some(256));
    }

    #[test]
    fn migrate_both_old_cache_keys() {
        let config = parse_yaml("cache_size_mb: 512\ntile_cache_size_mb: 256");
        assert_eq!(config.cache.size_mb, Some(512));
        assert_eq!(config.cache.tile_size_mb, Some(256));
    }

    #[test]
    fn new_cache_key_overrides_old() {
        let config = parse_yaml("cache_size_mb: 100\ncache:\n  size_mb: 200");
        assert_eq!(config.cache.size_mb, Some(200));
    }

    #[test]
    fn new_cache_format_works_directly() {
        let config =
            parse_yaml("cache:\n  size_mb: 512\n  tile_size_mb: 256\n  minzoom: 2\n  maxzoom: 10");
        assert_eq!(config.cache.size_mb, Some(512));
        assert_eq!(config.cache.tile_size_mb, Some(256));
    }

    #[cfg(feature = "sprites")]
    #[test]
    fn migrate_sprites_cache_size_mb() {
        let config = parse_yaml("sprites:\n  cache_size_mb: 64\n  paths: /tmp");
        let FileConfigEnum::Config(cfg) = &config.sprites else {
            panic!("expected sprites config");
        };
        assert_eq!(cfg.custom.cache.size_mb, Some(64));
    }

    #[cfg(feature = "fonts")]
    #[test]
    fn migrate_fonts_cache_size_mb() {
        let config = parse_yaml("fonts:\n  cache_size_mb: 32\n  paths: /tmp");
        let FileConfigEnum::Config(cfg) = &config.fonts else {
            panic!("expected fonts config");
        };
        assert_eq!(cfg.custom.cache.size_mb, Some(32));
    }

    #[test]
    fn migrate_skips_non_mapping_intermediate() {
        // `cache: true` is not a mapping, so migration of cache_size_mb should
        // gracefully skip rather than panic, and the parse should still succeed
        // (cache will be deserialized from whatever value it has).
        let result = parse_config(
            "cache: true\ncache_size_mb: 100",
            &HashMap::<String, String>::new(),
            Path::new("test.yaml"),
        );
        // The parse may fail (cache: true is not a valid GlobalCacheConfig),
        // but it must not panic.
        let _ = result;
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
            other => panic!("expected exactly one postgres config, got: {other:?}"),
        };
        assert_eq!(pg.connection_string.as_deref(), expected);
    }

    #[rstest]
    #[case::var_set("${BASE:fallback}", "/my/path")]
    #[case::var_unset("${UNSET:fallback}", "fallback")]
    fn substitution_single_colon_default_is_translated(
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        let env = props(&[("BASE", "/my/path")]);
        let config = parse_with_env(&format!("base_path: {input}\n"), &env);
        assert_eq!(config.srv.base_path.as_deref(), Some(expected));
    }

    #[rstest]
    #[case::braced("base_path: ${BASE}\n")]
    #[case::bare("base_path: $BASE\n")]
    #[case::dash_default("base_path: ${BASE:-fallback}\n")]
    #[case::plus_alternate("base_path: ${BASE:+set}\n")]
    #[case::error_if_unset("connection_string: ${DB:?required}\n")]
    #[case::bare_dash("base_path: ${BASE-fallback}\n")]
    #[case::escaped_dollar("escaped: $$BASE\n")]
    #[case::escaped_braced("escaped: $${a:b}\n")]
    #[case::plain("plain: no substitution here\n")]
    fn rewrite_legacy_substitution_syntax_borrows_when_unchanged(#[case] input: &str) {
        assert!(
            matches!(rewrite_legacy_substitution_syntax(input), Cow::Borrowed(_)),
            "should not rewrite: {input:?}"
        );
    }

    #[rstest]
    #[case::simple("${BASE:fallback}", "${BASE:-fallback}")]
    #[case::colon_in_default("${UNSET:postgres://h:5432/db}", "${UNSET:-postgres://h:5432/db}")]
    #[case::nested("${a:x${b:y}}", "${a:-x${b:-y}}")]
    #[case::prefix_and_suffix("prefix-${BASE:def}-suffix", "prefix-${BASE:-def}-suffix")]
    #[case::escaped_prefix("$$${a:b}", "$$${a:-b}")]
    fn rewrite_legacy_substitution_syntax_translates_single_colon(
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        match rewrite_legacy_substitution_syntax(input) {
            Cow::Owned(s) => assert_eq!(s, expected, "input {input:?}"),
            Cow::Borrowed(_) => panic!("expected rewrite for {input:?}"),
        }
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn substitution_legacy_single_colon_connection_string() {
        let config = parse_with_env(
            "postgres:\n  connection_string: ${UNSET:postgres://postgres@localhost:5432/db}\n",
            &HashMap::new(),
        );
        let pg = match config.postgres {
            OptOneMany::One(pg) => pg,
            other => panic!("expected exactly one postgres config, got: {other:?}"),
        };
        assert_eq!(
            pg.connection_string.as_deref(),
            Some("postgres://postgres@localhost:5432/db")
        );
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
            other => panic!("expected exactly one postgres config, got: {other:?}"),
        };
        assert_eq!(
            one.connection_string.as_deref(),
            Some("postgres://postgres:postgres@db:5432/ehrenamtskarte"),
        );
    }

    #[rstest]
    #[case::braced_in_migrated_key(
        &[("SIZE", "512")],
        "cache_size_mb: ${SIZE}\n",
        Some(512), None, None,
    )]
    #[case::sibling_to_migrated_key(
        &[("SIZE", "256"), ("BASE", "/served")],
        "tile_cache_size_mb: ${SIZE}\nbase_path: ${BASE}\n",
        None, Some(256), Some("/served"),
    )]
    #[case::bare_in_migrated_key(
        &[("SIZE", "1024"), ("BASE", "/p")],
        "cache_size_mb: $SIZE\nbase_path: $BASE\n",
        Some(1024), None, Some("/p"),
    )]
    fn substitution_survives_deprecated_migration(
        #[case] env_pairs: &[(&str, &str)],
        #[case] yaml: &str,
        #[case] expected_size_mb: Option<u64>,
        #[case] expected_tile_size_mb: Option<u64>,
        #[case] expected_base_path: Option<&str>,
    ) {
        let env = props(env_pairs);
        let config = parse_with_env(yaml, &env);
        assert_eq!(config.cache.size_mb, expected_size_mb);
        assert_eq!(config.cache.tile_size_mb, expected_tile_size_mb);
        assert_eq!(config.srv.base_path.as_deref(), expected_base_path);
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn string_references_var_matches_substitution_tokens() {
        for s in [
            "${DATABASE_URL}",
            "${DATABASE_URL:-postgres://x}",
            "${DATABASE_URL:?required}",
            "${DATABASE_URL-no-default}",
            "${DATABASE_URL+set}",
            "${DATABASE_URL?msg}",
            "prefix-${DATABASE_URL}-suffix",
            "$DATABASE_URL",
            "$DATABASE_URL/path",
            "$DATABASE_URL ",
        ] {
            assert!(
                string_references_var(s, "DATABASE_URL"),
                "expected a hit in {s:?}"
            );
        }
        for s in [
            "DATABASE_URL",
            "$$DATABASE_URL",
            "$DATABASE_URLISH",
            "${DATABASE_URL_OTHER}",
            "${OTHER_DATABASE_URL}",
            "postgres://db",
        ] {
            assert!(
                !string_references_var(s, "DATABASE_URL"),
                "expected no hit in {s:?}"
            );
        }
    }
}
