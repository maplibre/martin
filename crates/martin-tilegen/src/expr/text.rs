//! Text functions for names, matching Planetiler's `LanguageUtils` character classes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

/// Planetiler's `ONLY_LATIN`: every character is a non-letter or a letter of the Latin script.
static ONLY_LATIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\P{L}[\p{L}&&\p{sc=Latin}]]+$").expect("the latin-only pattern compiles"));

/// Planetiler's `LATIN_LETTER`: a run of letters of the Latin script.
static LATIN_LETTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\p{L}&&\p{sc=Latin}]+").expect("the latin letter pattern compiles"));

/// Planetiler's `EMPTY_PARENS`: brackets holding only spaces and the punctuation from ` ` to `.`.
static EMPTY_PARENS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\([ -.]*\)|\[[ -.]*\]").expect("the empty brackets pattern compiles"));

/// Planetiler's `WHITESPACE`: a run of Java's ASCII `\s`.
static WHITESPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[ \t\n\x0B\x0C\r]+").expect("the whitespace pattern compiles"));

/// How many compiled patterns each thread keeps for `matches` and `regex.replace`.
const PATTERN_CACHE_SIZE: usize = 256;

thread_local! {
    /// The patterns `matches` and `regex.replace` compiled on this thread, by their source.
    static PATTERNS: RefCell<HashMap<String, Regex>> = RefCell::new(HashMap::new());
}

/// Whether `text` holds only non-letters and letters of the Latin script, as Planetiler's
/// `containsOnlyLatinCharacters`.
pub(super) fn is_latin(text: &str) -> bool {
    ONLY_LATIN.is_match(text)
}

/// `text` without its Latin letters and the brackets and dashes they leave behind, as Planetiler's
/// `removeLatinCharacters`, or `None` when nothing but blanks is left.
pub(super) fn remove_latin(text: &str) -> Option<String> {
    if !LATIN_LETTER.is_match(text) {
        return Some(java_trim(text).to_owned());
    }
    let result = LATIN_LETTER.replace_all(text, "");
    let result = EMPTY_PARENS.replace_all(&result, "");
    let result = result.trim_matches(|c| is_java_ascii_space(c) || matches!(c, '.' | '/' | '-'));
    let result = WHITESPACE.replace_all(result, " ");
    let result = java_trim(&result);
    (!result.chars().all(is_java_whitespace)).then(|| result.to_owned())
}

/// Whether `pattern` matches anywhere in `text`.
pub(super) fn is_match(text: &str, pattern: &str) -> Result<bool, String> {
    PATTERNS.with_borrow_mut(|patterns| {
        if let Some(compiled) = patterns.get(pattern) {
            return Ok(compiled.is_match(text));
        }
        let compiled = Regex::new(pattern).map_err(|e| format!("'{pattern}' not a valid regex:\n{e}"))?;
        let matched = compiled.is_match(text);
        if patterns.len() >= PATTERN_CACHE_SIZE {
            patterns.clear();
        }
        patterns.insert(pattern.to_owned(), compiled);
        Ok(matched)
    })
}

/// `text` with each match of `pattern` replaced by `replacement`, where `\1` to `\9` insert a group,
/// `\0` the match and `\\` a backslash, as cel-go's `regex.replace`.
pub(super) fn replace_all(text: &str, pattern: &str, replacement: &str) -> Result<String, String> {
    let replacement = expand_groups(replacement)?;
    PATTERNS.with_borrow_mut(|patterns| {
        if let Some(compiled) = patterns.get(pattern) {
            return Ok(compiled.replace_all(text, replacement.as_str()).into_owned());
        }
        let compiled = Regex::new(pattern).map_err(|e| format!("'{pattern}' not a valid regex:\n{e}"))?;
        let replaced = compiled.replace_all(text, replacement.as_str()).into_owned();
        if patterns.len() >= PATTERN_CACHE_SIZE {
            patterns.clear();
        }
        patterns.insert(pattern.to_owned(), compiled);
        Ok(replaced)
    })
}

/// `replacement` in the `regex` crate's syntax: `\N` becomes `${N}` and `$` is escaped.
fn expand_groups(replacement: &str) -> Result<String, String> {
    let mut expanded = String::with_capacity(replacement.len());
    let mut chars = replacement.chars();
    while let Some(c) = chars.next() {
        match c {
            '$' => expanded.push_str("$$"),
            '\\' => match chars.next() {
                Some('\\') => expanded.push('\\'),
                Some(digit @ '0'..='9') => {
                    expanded.push_str("${");
                    expanded.push(digit);
                    expanded.push('}');
                }
                Some(other) => return Err(format!("invalid replacement escape `\\{other}`")),
                None => return Err("replacement ends with a lone `\\`".to_owned()),
            },
            c => expanded.push(c),
        }
    }
    Ok(expanded)
}

/// Java's `String.trim`: drops the characters up to U+0020 at both ends.
fn java_trim(text: &str) -> &str {
    text.trim_matches(|c| c <= ' ')
}

/// Java's ASCII `\s`.
fn is_java_ascii_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0B' | '\x0C' | '\r')
}

/// Java's `Character.isWhitespace`, which `String.isBlank` tests.
fn is_java_whitespace(c: char) -> bool {
    matches!(c, '\t'..='\r' | '\u{1C}'..='\u{1F}')
        || (c.is_whitespace() && !matches!(c, '\u{85}' | '\u{A0}' | '\u{2007}' | '\u{202F}'))
}
