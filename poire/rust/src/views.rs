//! RS-13 owned-parameter habits, RS-14 stale idioms, RS-27 borrow-or-own.

use std::borrow::Cow;
use std::fmt;

/// Counts the words in `text` longer than `min` characters.
///
/// Takes `&str`, so a caller can pass a `String`, a literal or a slice without a copy.
#[must_use]
pub fn count_long_words(text: &str, min: usize) -> usize {
    text.split_whitespace()
        .filter(|word| word.chars().count() > min)
        .count()
}

/// Trims `input` and collapses each run of whitespace to one space.
///
/// Borrows when the input is already in that form, and allocates once when it is not.
#[must_use]
pub fn squeeze_spaces(input: &str) -> Cow<'_, str> {
    if is_squeezed(input) {
        return Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len());
    for word in input.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    Cow::Owned(out)
}

fn is_squeezed(s: &str) -> bool {
    // Start as if after a space, so a leading space counts as a run.
    let mut after_space = true;
    for c in s.chars() {
        let space = c.is_whitespace();
        if space && (after_space || c != ' ') {
            return false;
        }
        after_space = space;
    }
    s.is_empty() || !after_space
}

/// One `key=value` line of a settings file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setting<'a> {
    /// The trimmed key.
    pub key: &'a str,
    /// The trimmed value.
    pub value: &'a str,
}

/// Why a settings line did not parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettingError {
    /// The line has no `=`.
    MissingEquals,
    /// The key before `=` is empty.
    EmptyKey,
}

impl fmt::Display for SettingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingEquals => "setting has no '='",
            Self::EmptyKey => "setting has an empty key",
        })
    }
}

impl std::error::Error for SettingError {}

/// Parses one settings line. Blank lines and `#` comments give `Ok(None)`.
///
/// # Errors
/// [`SettingError`] when a non-comment line is not `key=value` with a key.
pub fn parse_setting(line: &str) -> Result<Option<Setting<'_>>, SettingError> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let Some((key, value)) = line.split_once('=') else {
        return Err(SettingError::MissingEquals);
    };
    let key = key.trim();
    if key.is_empty() {
        return Err(SettingError::EmptyKey);
    }
    Ok(Some(Setting {
        key,
        value: value.trim(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squeeze_borrows_clean_input_and_fixes_the_rest() {
        const CASES: &[(&str, &str, bool)] = &[
            ("", "", true),
            ("a b", "a b", true),
            (" a", "a", false),
            ("a ", "a", false),
            ("a  b", "a b", false),
            ("a\tb", "a b", false),
        ];
        for &(input, want, borrowed) in CASES {
            let got = squeeze_spaces(input);
            assert_eq!(got, want, "input {input:?}");
            assert_eq!(
                matches!(got, Cow::Borrowed(_)),
                borrowed,
                "borrow state for {input:?}"
            );
        }
    }

    #[test]
    fn settings_parse_or_say_why_not() {
        assert_eq!(parse_setting("  # note"), Ok(None), "comment");
        assert_eq!(
            parse_setting(" port = 80 "),
            Ok(Some(Setting {
                key: "port",
                value: "80"
            })),
            "trimmed pair"
        );
        assert_eq!(
            parse_setting("port"),
            Err(SettingError::MissingEquals),
            "no '='"
        );
        assert_eq!(parse_setting(" =80"), Err(SettingError::EmptyKey), "no key");
    }

    #[test]
    fn errors_say_what_happened() {
        assert_eq!(
            SettingError::MissingEquals.to_string(),
            "setting has no '='",
            "no '='"
        );
        assert_eq!(
            SettingError::EmptyKey.to_string(),
            "setting has an empty key",
            "no key"
        );
    }

    proptest::proptest! {
        #[test]
        fn property_squeezing_is_idempotent(input in "[ a-z\t\n]{0,24}") {
            let once = squeeze_spaces(&input).into_owned();
            let twice = squeeze_spaces(&once);
            proptest::prop_assert!(
                matches!(twice, Cow::Borrowed(_)),
                "squeezed text is left alone: {:?}", once
            );
            proptest::prop_assert_eq!(twice, once.as_str());
        }
    }

    #[test]
    fn long_words_count_characters_not_bytes() {
        assert_eq!(count_long_words("ab abc ééé", 2), 2, "é is one character");
    }
}
