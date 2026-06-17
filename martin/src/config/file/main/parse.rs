use std::path::Path;

use subst::VariableMap;
use tracing::warn;

use super::Config;
use crate::config::file::{ConfigFileError, ConfigFileResult};

/// Read config from a file
pub fn read_config<'a, M>(file_name: &Path, env: &'a M) -> ConfigFileResult<Config>
where
    M: VariableMap<'a>,
    M::Value: AsRef<str>,
{
    let contents = std::fs::read_to_string(file_name)
        .map_err(|e| ConfigFileError::ConfigLoadError(e, file_name.into()))?;
    parse_config(&contents, env, file_name)
}

pub fn parse_config<'a, M>(contents: &str, env: &'a M, file_name: &Path) -> ConfigFileResult<Config>
where
    M: VariableMap<'a>,
    M::Value: AsRef<str>,
{
    // Phase 1: substitute environment variables at the text level so saphyr's spans line up
    // with the post-substitution text the parser actually sees.
    let substituted = subst::substitute(contents, env)
        .map_err(|e| ConfigFileError::substitution(e, contents.to_string(), file_name))?;

    // Phase 2: rewrite deprecated cache keys via a `serde_yaml::Value` round-trip - but only
    // if at least one deprecated token appears in the text. The common case (no deprecated
    // keys) skips a full YAML parse + serialize.
    let migrated = if needs_deprecated_migration(&substituted) {
        match serde_yaml::from_str::<serde_yaml::Value>(&substituted) {
            Ok(mut value) => {
                migrate_deprecated_config(&mut value);
                serde_yaml::to_string(&value).unwrap_or(substituted)
            }
            // If serde_yaml itself can't parse, hand the original to saphyr - its diagnostics
            // are richer, so let it produce the user-facing error.
            Err(_) => substituted,
        }
    } else {
        substituted
    };

    // Phase 3: parse to the typed `Config` via saphyr. We disable saphyr's built-in snippet
    // wrapper so its hardcoded `<input>` source name doesn't override the file path we show;
    // `ConfigFileError::to_miette_report` re-attaches a snippet against our own NamedSource.
    let options = serde_saphyr::options! {
        with_snippet: false,
    };
    match serde_saphyr::from_str_with_options::<Config>(&migrated, options) {
        Ok(config) => Ok(config),
        Err(e) => {
            // A malformed `connection_string` is flagged during deserialization with a sentinel
            // marker (see `deserialize_connection_string`). Rebuild it as a snippet-less,
            // password-redacted diagnostic that points at the value's location, so the raw line
            // is never echoed. Other parse errors keep their normal source-snippet rendering.
            #[cfg(feature = "postgres")]
            if let Some(err) = invalid_connection_string_error(&e, &migrated, file_name) {
                return Err(err);
            }
            Err(ConfigFileError::yaml_parse(e, migrated, file_name))
        }
    }
}

/// If `error` is the sentinel a malformed `connection_string` produces, rebuild it as a
/// [`ConfigFileError::InvalidConnectionString`]: pull the value's location from the parse error,
/// slice it out of `source`, and re-validate to get a password-redacted message. The raw value is
/// only used to recompute the redaction — it is never stored or rendered. Returns `None` for any
/// other error, or if the location/slice is unavailable (caller then renders the plain error).
#[cfg(feature = "postgres")]
fn invalid_connection_string_error(
    error: &serde_saphyr::Error,
    source: &str,
    file_name: &Path,
) -> Option<ConfigFileError> {
    use martin_core::tiles::postgres::{redact_conn_str, validate_conn_str};

    use crate::config::file::error::INVALID_CONN_STR_MARKER;

    if !error.to_string().contains(INVALID_CONN_STR_MARKER) {
        return None;
    }
    let location = error.location()?;
    let span = location.span();
    let offset = usize::try_from(span.byte_offset()?).ok()?;
    let len = usize::try_from(span.byte_len()?).ok()?;
    let value = source.get(offset..offset.checked_add(len)?)?;
    // Re-run validation to recover the same password-redacted message; fall back to a redacted
    // value if it somehow parses now.
    let message = validate_conn_str(value).err().map_or_else(
        || format!("invalid connection string {}", redact_conn_str(value)),
        |e| e.to_string(),
    );
    Some(ConfigFileError::invalid_connection_string(
        message,
        file_name,
        location.line(),
        location.column(),
    ))
}

/// Cheap pre-check: does the substituted YAML mention any deprecated cache key?
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
/// This runs on the `serde_yaml::Value` directly, so the `Config` struct
/// never needs to know about deprecated field names.
fn migrate_deprecated_config(value: &mut serde_yaml::Value) {
    let Some(root) = value.as_mapping_mut() else {
        return;
    };

    // Global: cache_size_mb -> cache.size_mb
    migrate_yaml_key(root, "cache_size_mb", &["cache", "size_mb"]);

    // Global: tile_cache_size_mb -> cache.tile_size_mb
    migrate_yaml_key(root, "tile_cache_size_mb", &["cache", "tile_size_mb"]);

    // Source-type level: {section}.cache_size_mb -> {section}.cache.size_mb
    for section in ["sprites", "fonts"] {
        if let Some(mapping) = root
            .get_mut(serde_yaml::Value::String(section.into()))
            .and_then(|v| v.as_mapping_mut())
        {
            migrate_yaml_key(mapping, "cache_size_mb", &["cache", "size_mb"]);
        }
    }

    // PMTiles: directory_cache_size_mb -> directory_cache.size_mb
    if let Some(mapping) = root
        .get_mut(serde_yaml::Value::String("pmtiles".into()))
        .and_then(|v| v.as_mapping_mut())
    {
        migrate_yaml_key(
            mapping,
            "directory_cache_size_mb",
            &["directory_cache", "size_mb"],
        );
    }
}

/// Moves a deprecated key in a YAML mapping to a new nested location.
///
/// `new_path` is a slice of keys describing the nested destination,
/// e.g. `&["cache", "size_mb"]` means `cache.size_mb`.
///
/// If the new key already exists, the old value is dropped with a warning.
/// If only the old key exists, it is moved to the new location.
fn migrate_yaml_key(mapping: &mut serde_yaml::Mapping, old_key: &str, new_path: &[&str]) {
    debug_assert!(!new_path.is_empty(), "new_path must not be empty");

    let old_yaml_key = serde_yaml::Value::String(old_key.into());
    let Some(old_value) = mapping.remove(&old_yaml_key) else {
        return;
    };

    let new_key_display = new_path.join(".");

    // Walk down to the parent of the leaf key, creating intermediate mappings as needed
    let [parents @ .., leaf] = new_path else {
        return;
    };
    let mut current = &mut *mapping;
    for &segment in parents {
        if !current.contains_key(segment) {
            current.insert(
                serde_yaml::Value::String(segment.into()),
                serde_yaml::Value::Mapping(serde_yaml::Mapping::default()),
            );
        }
        let Some(nested) = current.get_mut(segment).and_then(|v| v.as_mapping_mut()) else {
            warn!(
                "deprecated config: `{old_key}` is ignored because `{segment}` is already set. \
                 Please remove `{old_key}` from your configuration"
            );
            return;
        };
        current = nested;
    }

    if current.contains_key(leaf) {
        warn!(
            "deprecated config: `{old_key}` is ignored in favor of `{new_key_display}`. \
             Please remove `{old_key}` from your configuration"
        );
    } else {
        current.insert(serde_yaml::Value::String((*leaf).into()), old_value);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::path::Path;
    use std::time::Duration;

    use rstest::rstest;

    use super::*;
    #[cfg(any(feature = "sprites", feature = "fonts"))]
    use crate::config::file::FileConfigEnum;
    use crate::config::file::{CachePolicy, Config, GlobalCacheConfig};
    use crate::config::primitives::env::FauxEnv;
    use crate::config::test_helpers::{render_failure, render_failure_json};

    fn parse_yaml(yaml: &str) -> Config {
        parse_config(
            yaml,
            &HashMap::<String, String>::new(),
            Path::new("test.yaml"),
        )
        .unwrap()
    }

    fn faux_env(pairs: &[(&'static str, &str)]) -> FauxEnv {
        FauxEnv(
            pairs
                .iter()
                .map(|(k, v)| (*k, OsString::from(*v)))
                .collect(),
        )
    }

    fn parse_with_env(yaml: &str, env: &FauxEnv) -> Config {
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
         × invalid indentation in multiline quoted scalar
          ╭─[config.yaml:3:3]
        2 │   listen_addresses: "0.0.0.0:3000
        3 │   worker_processes: 4
          ·   ┬
          ·   ╰── invalid indentation in multiline quoted scalar
          ╰────
        "#
        );
    }

    #[test]
    fn unknown_enum_variant_in_on_invalid() {
        insta::assert_snapshot!(render_failure("on_invalid: maybe\n"), @"
         × unknown variant `maybe`, expected one of continue, ignore, warn, warning,
         │ warnings, abort
          ╭─[config.yaml:1:13]
        1 │ on_invalid: maybe
          ·             ──┬──
          ·               ╰── unknown variant `maybe`, expected one of continue, ignore, warn, warning, warnings, abort
          ╰────
        ");
    }

    #[test]
    fn substitution_undefined_variable() {
        insta::assert_snapshot!(render_failure("cache_size_mb: ${UNDEFINED_VAR}\n"), @"
        martin::config::substitution (https://maplibre.org/martin/config-file/)

          × Unable to substitute environment variables in config file config.yaml: No
          │ such variable: $UNDEFINED_VAR
           ╭─[config.yaml:1:18]
         1 │ cache_size_mb: ${UNDEFINED_VAR}
           ·                  ──────┬──────
           ·                        ╰── No such variable: $UNDEFINED_VAR
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
    fn substitution_renders_as_json_with_code_help_url() {
        // The substitution path uses our own `SubstitutionDiagnostic`, which overrides
        // `code()`, `help()`, and `url()`. The JSON renderer surfaces all three.
        let json = render_failure_json("cache_size_mb: ${UNDEFINED_VAR}\n");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).unwrap_or_else(|e| panic!("not JSON: {e}\n{json}"));

        assert_eq!(
            parsed.get("code").and_then(|c| c.as_str()),
            Some("martin::config::substitution")
        );
        let help = parsed.get("help").and_then(|h| h.as_str()).unwrap_or("");
        assert!(
            help.contains("${VAR}"),
            "expected help text mentioning ${{VAR}}, got: {help}"
        );
        assert_eq!(
            parsed.get("url").and_then(|u| u.as_str()),
            Some("https://maplibre.org/martin/config-file/")
        );
    }

    #[test]
    fn substitution_unclosed_brace() {
        insta::assert_snapshot!(render_failure("cache_size_mb: ${BROKEN\n"), @r"
        martin::config::substitution (https://maplibre.org/martin/config-file/)

          × Unable to substitute environment variables in config file config.yaml:
          │ Unexpected character: '\n', expected a closing brace ('}') or colon (':')
           ╭─[config.yaml:1:24]
         1 │ cache_size_mb: ${BROKEN
           ·                        ┬
           ·                        ╰── Unexpected character: '\n', expected a closing brace ('}') or colon (':')
           ╰────
          help: Make sure every ${VAR} reference resolves to an environment variable,
                or supply a default with `${VAR:-fallback}`.
        ");
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
        // Parse a global `cache: disable` and verify it propagates to a source with no cache config
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
    #[case::braced_with_default_var_present("${BASE:fallback}", "/my/path")]
    #[case::default_used_when_var_unset("${UNSET:/fallback}", "/fallback")]
    #[case::prefix_and_suffix("prefix-${BASE}-suffix", "prefix-/my/path-suffix")]
    #[case::escape_dollar(r"\$BASE", "$BASE")]
    #[case::escape_brace(r"\${BASE}", "${BASE}")]
    fn substitution_subst_accepted_forms(#[case] input: &str, #[case] expected: &str) {
        let env = faux_env(&[("BASE", "/my/path")]);
        let yaml = format!("base_path: {input}\n");
        let config = parse_with_env(&yaml, &env);
        assert_eq!(config.srv.base_path.as_deref(), Some(expected));
    }

    #[rstest]
    #[case::dash_default("${UNSET:-fallback}", "-fallback")]
    #[case::plus_alternate("${UNSET:+set}", "+set")]
    #[case::question_required("${UNSET:?required}", "?required")]
    fn substitution_subst_treats_shell_operators_as_literal(
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        let env = FauxEnv::default();
        let yaml = format!("base_path: {input}\n");
        let config = parse_with_env(&yaml, &env);
        assert_eq!(config.srv.base_path.as_deref(), Some(expected));
    }

    #[rstest]
    #[case::unquoted("base_path: ${BASE}\n")]
    #[case::single_quoted("base_path: '${BASE}'\n")]
    #[case::double_quoted("base_path: \"${BASE}\"\n")]
    fn substitution_subst_interpolates_regardless_of_yaml_quotes(#[case] yaml: &str) {
        let env = faux_env(&[("BASE", "/my/path")]);
        let config = parse_with_env(yaml, &env);
        assert_eq!(config.srv.base_path.as_deref(), Some("/my/path"));
    }

    #[test]
    fn substitution_subst_errors_on_unset_var_inside_comment() {
        insta::assert_snapshot!(
            render_failure("# ${UNSET_IN_COMMENT}\nbase_path: /static\n"),
            @"
        martin::config::substitution (https://maplibre.org/martin/config-file/)

          × Unable to substitute environment variables in config file config.yaml: No
          │ such variable: $UNSET_IN_COMMENT
           ╭─[config.yaml:1:5]
         1 │ # ${UNSET_IN_COMMENT}
           ·     ────────┬───────
           ·             ╰── No such variable: $UNSET_IN_COMMENT
         2 │ base_path: /static
           ╰────
          help: Make sure every ${VAR} reference resolves to an environment variable,
                or supply a default with `${VAR:-fallback}`.
        "
        );
    }

    #[test]
    fn substitution_subst_silently_substitutes_inside_comment() {
        let env = faux_env(&[("DEFINED_IN_COMMENT", "anything")]);
        let config = parse_with_env("# ${DEFINED_IN_COMMENT}\nbase_path: /x\n", &env);
        assert_eq!(config.srv.base_path.as_deref(), Some("/x"));
    }

    #[test]
    fn substitution_empty_default_with_unset_var_becomes_yaml_null() {
        let env = FauxEnv::default();
        let config = parse_with_env("base_path: ${UNSET:}\n", &env);
        assert_eq!(config.srv.base_path, None);
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
        #[case] env_pairs: &[(&'static str, &str)],
        #[case] yaml: &str,
        #[case] expected_size_mb: Option<u64>,
        #[case] expected_tile_size_mb: Option<u64>,
        #[case] expected_base_path: Option<&str>,
    ) {
        let env = faux_env(env_pairs);
        let config = parse_with_env(yaml, &env);
        assert_eq!(config.cache.size_mb, expected_size_mb);
        assert_eq!(config.cache.tile_size_mb, expected_tile_size_mb);
        assert_eq!(config.srv.base_path.as_deref(), expected_base_path);
    }

    #[test]
    fn substitution_rejects_hyphen_in_variable_name() {
        insta::assert_snapshot!(
            render_failure("base_path: ${ab-cd}\n"),
            @"
        martin::config::substitution (https://maplibre.org/martin/config-file/)

          × Unable to substitute environment variables in config file config.yaml:
          │ Unexpected character: '-', expected a closing brace ('}') or colon (':')
           ╭─[config.yaml:1:16]
         1 │ base_path: ${ab-cd}
           ·                ┬
           ·                ╰── Unexpected character: '-', expected a closing brace ('}') or colon (':')
           ╰────
          help: Make sure every ${VAR} reference resolves to an environment variable,
                or supply a default with `${VAR:-fallback}`.
        "
        );
    }

    #[test]
    fn substitution_double_dollar_is_not_an_escape() {
        insta::assert_snapshot!(
            render_failure("base_path: $$BASE\n"),
            @"
        martin::config::substitution (https://maplibre.org/martin/config-file/)

          × Unable to substitute environment variables in config file config.yaml:
          │ Missing variable name
           ╭─[config.yaml:1:12]
         1 │ base_path: $$BASE
           ·            ┬
           ·            ╰── Missing variable name
           ╰────
          help: Make sure every ${VAR} reference resolves to an environment variable,
                or supply a default with `${VAR:-fallback}`.
        "
        );
    }

    #[test]
    fn substitution_failure_in_comment_renders_as_json() {
        let json = render_failure_json("# ${UNSET_IN_COMMENT}\nbase_path: /x\n");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).unwrap_or_else(|e| panic!("not JSON: {e}\n{json}"));
        assert_eq!(
            parsed.get("code").and_then(|c| c.as_str()),
            Some("martin::config::substitution"),
        );
    }
}
