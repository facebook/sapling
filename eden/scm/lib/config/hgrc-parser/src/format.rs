/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Format Sapling config fragments so untrusted values cannot introduce new
//! directives.

/// A config fragment cannot represent its input safely.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid config {0}: cannot contain control characters")]
    ControlCharacters(&'static str),
    #[error("invalid config {0}: cannot be empty")]
    EmptyName(&'static str),
    #[error("invalid config {label}: cannot contain {forbidden}")]
    InvalidName {
        label: &'static str,
        forbidden: &'static str,
    },
    #[error("invalid config item name: cannot start with {0:?}")]
    ItemPrefix(char),
    #[error("invalid config value: cannot contain blank lines")]
    BlankValueLine,
}

fn validate_name<'a>(
    label: &'static str,
    name: &'a str,
    forbidden: &'static str,
) -> Result<&'a str, Error> {
    if name.contains(|c: char| c.is_ascii_control() && c != '\t') {
        return Err(Error::ControlCharacters(label));
    }
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::EmptyName(label));
    }
    if forbidden.chars().any(|c| name.contains(c)) {
        return Err(Error::InvalidName { label, forbidden });
    }
    Ok(name)
}

/// Validate and trim an item name before deriving names with prefixes or suffixes.
pub fn validate_config_item_name(name: &str) -> Result<&str, Error> {
    let name = validate_name("item name", name, "=")?;
    if let Some(prefix @ ('#' | '%' | ';' | '[')) = name.chars().next() {
        return Err(Error::ItemPrefix(prefix));
    }
    Ok(name)
}

/// Normalize names for looking up existing entries, including unsectioned keys.
/// Section-header syntax is checked separately when a header is formatted.
pub fn normalize_config_key<'a>(
    section: &'a str,
    name: &'a str,
) -> Result<(&'a str, &'a str), Error> {
    let section = if section.is_empty() {
        section
    } else {
        validate_name("section name", section, "")?
    };
    Ok((section, validate_config_item_name(name)?))
}

/// Validate and format a value, trimming trailing whitespace and indenting newlines.
pub fn format_config_value(value: &str) -> Result<String, Error> {
    if value.contains(['\0', '\r']) {
        return Err(Error::ControlCharacters("value"));
    }
    // The Python parser ends the value at a blank continuation line. An empty
    // first line follows "name =", so it still starts a multi-line value.
    if value
        .trim_end()
        .split('\n')
        .skip(1)
        .any(|line| line.trim().is_empty())
    {
        return Err(Error::BlankValueLine);
    }
    Ok(value.trim_end().replace('\n', "\n  "))
}

/// Format `name = value`, writing embedded newlines as indented continuation
/// lines so the value cannot start a new section or item.
pub fn format_config_item(name: &str, value: &str) -> Result<String, Error> {
    let name = validate_config_item_name(name)?;
    Ok(format!("{name} = {}\n", format_config_value(value)?))
}

/// Format a `[section]` header followed by one item.
pub fn format_config_section(section: &str, name: &str, value: &str) -> Result<String, Error> {
    let section = validate_name("section name", section, "[]")?;
    Ok(format!("[{section}]\n{}", format_config_item(name, value)?))
}

/// Format a `%include` directive.
pub fn format_config_include(path: &str) -> Result<String, Error> {
    let path = validate_name("include path", path, "")?;
    Ok(format!("%include {path}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_config_section_indents_continuations() {
        assert_eq!(
            format_config_section(
                " paths\t",
                " default[1] ",
                "https://example.invalid/repo\n[extensions]\nfoo = python:evil.py"
            )
            .expect("config section should format"),
            "[paths]\ndefault[1] = https://example.invalid/repo\n  [extensions]\n  foo = python:evil.py\n"
        );
    }

    #[test]
    fn test_rejects_invalid_config_syntax() {
        assert!(format_config_section("\npaths", "default", "value").is_err());
        assert!(format_config_section("paths", "default\nx", "value").is_err());
        assert!(format_config_section("paths", "default=x", "value").is_err());
        assert!(format_config_section("paths", "[extensions]", "value").is_err());
        assert!(format_config_section("paths", "#default", "value").is_err());
        assert!(format_config_section("paths", "default", "value\rnext").is_err());
        assert!(format_config_include(" \t ").is_err());
        assert!(format_config_include("/tmp/config\n[extensions]").is_err());
    }

    #[test]
    fn test_rejects_blank_continuation_lines() {
        for value in ["first\n\nsecond", "first\n  \nsecond"] {
            assert!(format_config_section("paths", "default", value).is_err());
        }
    }

    #[test]
    fn test_trims_trailing_whitespace() {
        assert_eq!(
            format_config_section("paths", "default", "value \n")
                .expect("config section should format"),
            "[paths]\ndefault = value\n"
        );
        assert_eq!(
            format_config_include(" \t/tmp/config #; file \t").expect("include should format"),
            "%include /tmp/config #; file\n"
        );
    }

    #[test]
    fn test_accepts_blank_first_line() {
        assert_eq!(
            format_config_section("paths", "default", "\nsecond")
                .expect("config section should format"),
            "[paths]\ndefault = \n  second\n"
        );
    }
}
