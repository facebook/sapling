/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Edit single items in hgrc-style config files in place, leaving the rest
//! of the file (comments, blank lines, other items) untouched.

use std::fs;
use std::io;
use std::io::Write;
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;

use hgrc_parser::Instruction;
use hgrc_parser::format;
use hgrc_parser::format::format_config_section;
use hgrc_parser::format::format_config_value;
use hgrc_parser::format::normalize_config_key;
use util::errors::IOContext;

/// Line ending written into config files.
const EOL: &str = if cfg!(windows) { "\r\n" } else { "\n" };

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Format(#[from] format::Error),
    #[error("cannot parse config file '{}': {message}", path.display())]
    Parse { path: PathBuf, message: String },
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// What [`edit_config`] found in the file before changing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditOutcome {
    /// Number of statements that set or unset the item. Only the last one
    /// is edited, so more than one means the file has duplicate entries.
    pub definitions: usize,
}

/// Set (`Some(value)`) or delete (`None`) the config item `section.name`
/// in the config file at `path`.
///
/// Setting replaces the value of the last existing definition in place,
/// or adds the item after the last item of its section, or appends a new
/// section when the section does not exist. Deleting removes the last
/// existing definition. Whitespace around `section` and `name` is ignored.
/// Files pulled in by `%include` are not followed.
///
/// Values are formatted so they cannot introduce new config statements.
/// A missing file is created. Writes are atomic and preserve the file's
/// permissions; nothing is written when the content would not change.
pub fn edit_config(
    path: &Path,
    section: &str,
    name: &str,
    value: Option<&str>,
) -> Result<EditOutcome, Error> {
    let (section, name) = normalize_config_key(section, name)?;
    let path = resolve_symlinks(path);
    let content = read_config(&path)?;
    let statements = parse(&path, &content)?;
    let definitions: Vec<&Statement> = statements
        .iter()
        .filter(|s| s.section == section && s.name == name)
        .collect();
    let outcome = EditOutcome {
        definitions: definitions.len(),
    };

    let new_content = match (value, definitions.last()) {
        (Some(value), Some(last)) => replace_value(&content, last, name, value)?,
        (Some(value), None) => {
            let section_end = statements
                .iter()
                .filter(|s| s.section == section)
                .map(|s| s.span.end)
                .max();
            match section_end {
                Some(end) => insert_item(&content, end, name, value)?,
                None => append_section(&content, section, name, value)?,
            }
        }
        (None, Some(last)) => remove_statement(&content, last),
        (None, None) => return Ok(outcome),
    };

    if new_content != content {
        write_config(&path, &new_content)?;
    }
    Ok(outcome)
}

/// A `name = value` or `%unset name` statement in the file.
struct Statement<'a> {
    section: &'a str,
    name: &'a str,
    /// Byte range of the value, or of the name for `%unset`.
    span: Range<usize>,
    unset: bool,
}

fn parse<'a>(path: &Path, content: &'a str) -> Result<Vec<Statement<'a>>, Error> {
    let instructions = hgrc_parser::parse(content).map_err(|err| Error::Parse {
        path: path.to_path_buf(),
        message: err.to_string(),
    })?;
    Ok(instructions
        .into_iter()
        .filter_map(|inst| match inst {
            Instruction::SetConfig {
                section,
                name,
                span,
                ..
            } => Some(Statement {
                section,
                name,
                span,
                unset: false,
            }),
            Instruction::UnsetConfig {
                section,
                name,
                span,
            } => Some(Statement {
                section,
                name,
                span,
                unset: true,
            }),
            Instruction::Include { .. } => None,
        })
        .collect())
}

fn replace_value(
    content: &str,
    last: &Statement,
    name: &str,
    value: &str,
) -> Result<String, Error> {
    let value = native_eol(format_config_value(value)?);
    // `%unset name` and `name =` without a value are rewritten as whole lines.
    let (range, replacement) = if last.unset || last.span.is_empty() {
        let line = line_start(content, last.span.start)..line_end(content, last.span.end);
        (line, format!("{name} = {value}"))
    } else {
        (last.span.clone(), value)
    };
    Ok(format!(
        "{}{replacement}{}",
        &content[..range.start],
        &content[range.end..]
    ))
}

fn insert_item(content: &str, at: usize, name: &str, value: &str) -> Result<String, Error> {
    let value = native_eol(format_config_value(value)?);
    Ok(format!(
        "{}{EOL}{name} = {value}{}",
        &content[..at],
        &content[at..]
    ))
}

fn append_section(content: &str, section: &str, name: &str, value: &str) -> Result<String, Error> {
    let text = native_eol(format_config_section(section, name, value)?);
    let separator = if content.is_empty() { "" } else { EOL };
    Ok(format!("{content}{separator}{text}"))
}

/// Remove the statement together with the line break before it, so the
/// line break after it keeps separating the surrounding lines. A statement
/// on the first line keeps its trailing line break.
fn remove_statement(content: &str, last: &Statement) -> String {
    let start = match content[..last.span.start].rfind('\n') {
        Some(newline) => newline - usize::from(content.as_bytes()[..newline].ends_with(b"\r")),
        None => 0,
    };
    format!("{}{}", &content[..start], &content[last.span.end..])
}

fn line_start(content: &str, pos: usize) -> usize {
    content[..pos].rfind('\n').map_or(0, |newline| newline + 1)
}

/// End of the line containing `pos`, excluding its line break.
fn line_end(content: &str, pos: usize) -> usize {
    let end = content[pos..]
        .find('\n')
        .map_or(content.len(), |newline| pos + newline);
    end - usize::from(content.as_bytes()[..end].ends_with(b"\r"))
}

fn native_eol(text: String) -> String {
    if cfg!(windows) {
        text.replace('\n', EOL)
    } else {
        text
    }
}

/// Resolve symlinks so the atomic rename replaces the file a symlink points
/// to rather than the symlink itself. A missing file resolves its directory.
fn resolve_symlinks(path: &Path) -> PathBuf {
    if let Ok(resolved) = fs::canonicalize(path) {
        return resolved;
    }
    let dir = path.parent().and_then(|dir| fs::canonicalize(dir).ok());
    match (dir, path.file_name()) {
        (Some(dir), Some(name)) => dir.join(name),
        _ => path.to_path_buf(),
    }
}

fn read_config(path: &Path) -> Result<String, Error> {
    match fs::read_to_string(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        result => Ok(result.path_context("cannot read config file", path)?),
    }
}

fn write_config(path: &Path, content: &str) -> Result<(), Error> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).path_context("cannot create config directory", dir)?;
    }
    atomicfile::atomic_write(path, file_mode(path), false, |f| {
        f.write_all(content.as_bytes())
    })
    .path_context("cannot write config file", path)?;
    Ok(())
}

/// Permissions for the rewritten file: those of the existing file, or the
/// default for a new file.
#[cfg(unix)]
fn file_mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).map_or_else(
        |_| util::file::apply_umask(0o666),
        |meta| meta.permissions().mode() & 0o777,
    )
}

#[cfg(not(unix))]
fn file_mode(_path: &Path) -> u32 {
    0o666
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    /// A config file in a temporary directory.
    struct Fixture {
        dir: TempDir,
        path: PathBuf,
    }

    impl Fixture {
        /// Create the file with `content`, written with native line endings.
        fn new(content: &str) -> Self {
            let fixture = Self::missing();
            fs::write(&fixture.path, eol(content)).unwrap();
            fixture
        }

        /// A path to a file that does not exist yet.
        fn missing() -> Self {
            let dir = TempDir::with_prefix("rcutil.").unwrap();
            let path = dir.path().join("config");
            Self { dir, path }
        }

        fn edit(&self, section: &str, name: &str, value: Option<&str>) -> EditOutcome {
            edit_config(&self.path, section, name, value).expect("edit should succeed")
        }

        fn edit_err(&self, section: &str, name: &str, value: Option<&str>) -> Error {
            edit_config(&self.path, section, name, value).expect_err("edit should fail")
        }

        fn content(&self) -> String {
            fs::read_to_string(&self.path).unwrap()
        }

        /// Assert the file content, written with native line endings, and
        /// that `section.name` reads back as `expected_value` from it.
        fn assert_content(
            &self,
            expected: &str,
            section: &str,
            name: &str,
            expected_value: Option<&str>,
        ) {
            assert_eq!(self.content(), eol(expected));
            assert_eq!(
                value_of(&self.content(), section, name).as_deref(),
                expected_value,
                "parsed value of {section}.{name}"
            );
        }
    }

    fn eol(text: &str) -> String {
        text.replace('\n', EOL)
    }

    /// Effective value of `section.name` after parsing `content`.
    fn value_of(content: &str, section: &str, name: &str) -> Option<String> {
        hgrc_parser::parse(content)
            .expect("edited content should parse")
            .into_iter()
            .filter_map(|inst| match inst {
                Instruction::SetConfig {
                    section: s,
                    name: n,
                    value,
                    ..
                } if s == section && n == name => Some(Some(value.into_owned())),
                Instruction::UnsetConfig {
                    section: s,
                    name: n,
                    ..
                } if s == section && n == name => Some(None),
                _ => None,
            })
            .next_back()
            .flatten()
    }

    #[test]
    fn test_add_creates_missing_file_and_directory() {
        let fixture = Fixture::missing();
        let path = fixture.dir.path().join("sub").join("dir").join("config");
        let outcome = edit_config(&path, "sec", "name", Some("val")).unwrap();
        assert_eq!(outcome, EditOutcome { definitions: 0 });
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            eol("[sec]\nname = val\n")
        );
    }

    #[test]
    fn test_add_appends_new_section_after_blank_line() {
        let fixture = Fixture::new("[sec1]\nname1 = val\n");
        fixture.edit("sec2", "name1", Some("val"));
        fixture.assert_content(
            "[sec1]\nname1 = val\n\n[sec2]\nname1 = val\n",
            "sec2",
            "name1",
            Some("val"),
        );
    }

    #[test]
    fn test_add_to_file_without_trailing_newline() {
        let fixture = Fixture::new("[a]\nb = 1");
        fixture.edit("a", "c", Some("2"));
        fixture.assert_content("[a]\nb = 1\nc = 2", "a", "c", Some("2"));
        fixture.edit("x", "y", Some("3"));
        fixture.assert_content("[a]\nb = 1\nc = 2\n[x]\ny = 3\n", "x", "y", Some("3"));
    }

    #[test]
    fn test_add_inserts_after_last_item_of_section() {
        let fixture = Fixture::new("[a]\nx = 1\n# comment\ny = 2\n\n[b]\nz = 3\n");
        fixture.edit("a", "new", Some("4"));
        fixture.assert_content(
            "[a]\nx = 1\n# comment\ny = 2\nnew = 4\n\n[b]\nz = 3\n",
            "a",
            "new",
            Some("4"),
        );
    }

    #[test]
    fn test_add_inserts_after_multiline_value() {
        let fixture = Fixture::new("[a]\nx = 1\n  2\n\n[b]\nz = 3\n");
        fixture.edit("a", "y", Some("dont\nmessup"));
        fixture.assert_content(
            "[a]\nx = 1\n  2\ny = dont\n  messup\n\n[b]\nz = 3\n",
            "a",
            "y",
            Some("dont\nmessup"),
        );
    }

    #[test]
    fn test_add_inserts_after_unset_statement() {
        let fixture = Fixture::new("[a]\n%unset x\n\n[b]\nz = 3\n");
        fixture.edit("a", "y", Some("1"));
        fixture.assert_content("[a]\n%unset x\ny = 1\n\n[b]\nz = 3\n", "a", "y", Some("1"));
    }

    #[test]
    fn test_add_uses_last_block_of_repeated_section() {
        let fixture = Fixture::new("[a]\nx = 1\n[b]\ny = 2\n[a]\nz = 3\n");
        fixture.edit("a", "w", Some("4"));
        fixture.assert_content(
            "[a]\nx = 1\n[b]\ny = 2\n[a]\nz = 3\nw = 4\n",
            "a",
            "w",
            Some("4"),
        );
    }

    #[test]
    fn test_add_to_section_without_items_appends_new_section() {
        // Section headers without items are not tracked, so the section is
        // repeated rather than reused.
        let fixture = Fixture::new("[a]\n");
        fixture.edit("a", "b", Some("1"));
        fixture.assert_content("[a]\n\n[a]\nb = 1\n", "a", "b", Some("1"));
    }

    #[test]
    fn test_add_unsectioned_item() {
        let fixture = Fixture::new("name = first\n");
        fixture.edit("", "other", Some("second"));
        fixture.assert_content(
            "name = first\nother = second\n",
            "",
            "other",
            Some("second"),
        );
    }

    #[test]
    fn test_add_unsectioned_item_to_empty_file_is_rejected() {
        let fixture = Fixture::new("");
        assert!(matches!(
            fixture.edit_err("", "name", Some("v")),
            Error::Format(_)
        ));
        assert_eq!(fixture.content(), "");
    }

    #[test]
    fn test_update_replaces_value_in_place() {
        let fixture = Fixture::new("[a]\n# b = 1\nb = 2\n  second line\n# b = 3\n");
        let outcome = fixture.edit("a", "b", Some("4"));
        assert_eq!(outcome, EditOutcome { definitions: 1 });
        fixture.assert_content("[a]\n# b = 1\nb = 4\n# b = 3\n", "a", "b", Some("4"));
    }

    #[test]
    fn test_update_single_line_to_multiline_and_back() {
        let fixture = Fixture::new("[sec1]\nname1 = val\nname2 = other\n");
        fixture.edit("sec1", "name1", Some("foo\nbar"));
        fixture.assert_content(
            "[sec1]\nname1 = foo\n  bar\nname2 = other\n",
            "sec1",
            "name1",
            Some("foo\nbar"),
        );
        fixture.edit("sec1", "name1", Some("baz"));
        fixture.assert_content(
            "[sec1]\nname1 = baz\nname2 = other\n",
            "sec1",
            "name1",
            Some("baz"),
        );
    }

    #[test]
    fn test_update_keeps_spacing_around_equals() {
        let fixture = Fixture::new("[a]\nb=1\nc   =   2\n");
        fixture.edit("a", "b", Some("x"));
        fixture.edit("a", "c", Some("y"));
        fixture.assert_content("[a]\nb=x\nc   =   y\n", "a", "c", Some("y"));
    }

    #[test]
    fn test_update_empty_value_rewrites_line() {
        let fixture = Fixture::new("[a]\nb =\nc = \n");
        fixture.edit("a", "b", Some("1"));
        fixture.edit("a", "c", Some("2"));
        fixture.assert_content("[a]\nb = 1\nc = 2\n", "a", "b", Some("1"));
        assert_eq!(value_of(&fixture.content(), "a", "c").as_deref(), Some("2"));
    }

    #[test]
    fn test_update_to_empty_value() {
        let fixture = Fixture::new("[a]\nb = 1\nc = 2\n");
        fixture.edit("a", "b", Some(""));
        fixture.assert_content("[a]\nb = \nc = 2\n", "a", "b", Some(""));
    }

    #[test]
    fn test_update_edits_last_duplicate_only() {
        let fixture = Fixture::new("[foo]\nbar = one\n[foo]\nbar = two\n");
        let outcome = fixture.edit("foo", "bar", Some("three"));
        assert_eq!(outcome, EditOutcome { definitions: 2 });
        fixture.assert_content(
            "[foo]\nbar = one\n[foo]\nbar = three\n",
            "foo",
            "bar",
            Some("three"),
        );
    }

    #[test]
    fn test_update_replaces_unset_statement() {
        let fixture = Fixture::new("[a]\nb = 1\n%unset b\nc = 2\n");
        let outcome = fixture.edit("a", "b", Some("3"));
        assert_eq!(outcome, EditOutcome { definitions: 2 });
        fixture.assert_content("[a]\nb = 1\nb = 3\nc = 2\n", "a", "b", Some("3"));
    }

    #[cfg(unix)]
    #[test]
    fn test_unchanged_content_is_not_rewritten() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = Fixture::new("[a]\nb = 1\n");
        // A read-only directory makes any rewrite fail.
        fs::set_permissions(fixture.dir.path(), fs::Permissions::from_mode(0o500)).unwrap();

        fixture.edit("a", "b", Some("1"));
        fixture.edit("a", "b", Some("1 "));
        fixture.edit("a", "missing", None);

        fs::set_permissions(fixture.dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        fixture.assert_content("[a]\nb = 1\n", "a", "b", Some("1"));
    }

    #[test]
    fn test_delete_item_in_middle_of_section() {
        let fixture = Fixture::new("[a]\nx = 1\ny = 2\nz = 3\n");
        let outcome = fixture.edit("a", "y", None);
        assert_eq!(outcome, EditOutcome { definitions: 1 });
        fixture.assert_content("[a]\nx = 1\nz = 3\n", "a", "y", None);
    }

    #[test]
    fn test_delete_multiline_value() {
        let fixture = Fixture::new("[a]\nx = 1\n  2\n  3\ny = 4\n");
        fixture.edit("a", "x", None);
        fixture.assert_content("[a]\ny = 4\n", "a", "x", None);
    }

    #[test]
    fn test_delete_last_item_keeps_section_header() {
        let fixture = Fixture::new("[sec1]\nname2 = bar\n  baz\n");
        fixture.edit("sec1", "name2", None);
        fixture.assert_content("[sec1]\n", "sec1", "name2", None);
    }

    #[test]
    fn test_delete_item_on_last_line_without_trailing_newline() {
        let fixture = Fixture::new("[a]\nx = 1\ny = 2");
        fixture.edit("a", "y", None);
        fixture.assert_content("[a]\nx = 1", "a", "y", None);
    }

    #[test]
    fn test_delete_item_on_first_line_keeps_trailing_newline() {
        let fixture = Fixture::new("name = first\n[a]\nb = 1\n");
        fixture.edit("", "name", None);
        fixture.assert_content("\n[a]\nb = 1\n", "", "name", None);
    }

    #[test]
    fn test_delete_removes_last_duplicate_only() {
        let fixture = Fixture::new("[foo]\nbar = one\n[foo]\nbar = two\nbaz = 3\n");
        let outcome = fixture.edit("foo", "bar", None);
        assert_eq!(outcome, EditOutcome { definitions: 2 });
        fixture.assert_content(
            "[foo]\nbar = one\n[foo]\nbaz = 3\n",
            "foo",
            "bar",
            Some("one"),
        );
    }

    #[test]
    fn test_delete_removes_unset_statement() {
        let fixture = Fixture::new("[a]\n%unset b\nc = 1\n");
        fixture.edit("a", "b", None);
        fixture.assert_content("[a]\nc = 1\n", "a", "b", None);
    }

    #[test]
    fn test_delete_missing_item_changes_nothing() {
        let fixture = Fixture::new("[a]\nb = 1\n");
        let outcome = fixture.edit("a", "missing", None);
        assert_eq!(outcome, EditOutcome { definitions: 0 });
        fixture.assert_content("[a]\nb = 1\n", "a", "b", Some("1"));

        let missing = Fixture::missing();
        missing.edit("a", "b", None);
        assert!(!missing.path.exists(), "deleting must not create the file");
    }

    #[test]
    fn test_names_are_trimmed() {
        let fixture = Fixture::new("");
        fixture.edit(" paths ", " default ", Some("first"));
        fixture.assert_content(
            "[paths]\ndefault = first\n",
            "paths",
            "default",
            Some("first"),
        );
        let outcome = fixture.edit(" paths ", " default ", Some("second"));
        assert_eq!(outcome, EditOutcome { definitions: 1 });
        fixture.assert_content(
            "[paths]\ndefault = second\n",
            "paths",
            "default",
            Some("second"),
        );
        fixture.edit(" paths ", " default ", None);
        fixture.assert_content("[paths]\n", "paths", "default", None);
    }

    #[test]
    fn test_value_whitespace_is_normalized() {
        let fixture = Fixture::new("[sec1]\nname1 = val\n");
        fixture.edit("sec1", "name1", Some("val \t"));
        fixture.edit("sec1", "name2", Some("a\nb "));
        fixture.assert_content(
            "[sec1]\nname1 = val\nname2 = a\n  b\n",
            "sec1",
            "name2",
            Some("a\nb"),
        );
    }

    #[test]
    fn test_value_cannot_inject_statements() {
        let fixture = Fixture::new("");
        let value = "first\n[extensions]\nfoo = python:evil.py\n";
        fixture.edit("paths", "default", Some(value));
        let content = fixture.content();
        assert_eq!(
            value_of(&content, "paths", "default").as_deref(),
            Some("first\n[extensions]\nfoo = python:evil.py")
        );
        assert_eq!(value_of(&content, "extensions", "foo"), None);
    }

    #[test]
    fn test_invalid_names_and_values_leave_file_untouched() {
        let original = "[section]\noriginal = old\n";
        let cases = [
            ("\nsection", "name", "v"),
            ("bad]section", "name", "v"),
            ("", "name", "v"),
            ("section", "", "v"),
            ("section", "bad=name", "v"),
            ("section", " #badname ", "v"),
            ("section", "name", "bad\rvalue"),
            ("section", "name", "bad\0value"),
            ("section", "name", "first\n\nsecond"),
            ("section", "original", "first\n  \nsecond"),
        ];
        for (section, name, value) in cases {
            let fixture = Fixture::new(original);
            let err = fixture.edit_err(section, name, Some(value));
            assert!(
                matches!(err, Error::Format(_)),
                "{section:?}.{name:?}={value:?}: {err}"
            );
            assert_eq!(
                fixture.content(),
                eol(original),
                "{section:?}.{name:?}={value:?}"
            );
        }
    }

    #[test]
    fn test_invalid_name_is_rejected_for_delete() {
        let fixture = Fixture::new("[a]\nb = 1\n");
        assert!(matches!(fixture.edit_err("a", "", None), Error::Format(_)));
        assert!(matches!(
            fixture.edit_err("a", "b=c", None),
            Error::Format(_)
        ));
        fixture.assert_content("[a]\nb = 1\n", "a", "b", Some("1"));
    }

    #[test]
    fn test_unparsable_file_is_rejected() {
        let original = "[a]\nb = 1\nthis is not a config line\n";
        let fixture = Fixture::new(original);
        let err = fixture.edit_err("a", "b", Some("2"));
        assert!(matches!(err, Error::Parse { .. }), "{err}");
        assert!(err.to_string().contains("line 3"), "{err}");
        assert_eq!(fixture.content(), eol(original));
    }

    #[test]
    fn test_includes_are_not_followed() {
        let fixture = Fixture::new("%include other.rc\n");
        let other = fixture.dir.path().join("other.rc");
        fs::write(&other, "[a]\nb = 1\n").unwrap();

        let outcome = fixture.edit("a", "b", Some("2"));
        assert_eq!(outcome, EditOutcome { definitions: 0 });
        fixture.assert_content("%include other.rc\n\n[a]\nb = 2\n", "a", "b", Some("2"));
        assert_eq!(fs::read_to_string(&other).unwrap(), "[a]\nb = 1\n");
    }

    #[test]
    fn test_crlf_line_endings() {
        let fixture = Fixture::missing();
        fs::write(&fixture.path, "[a]\r\nb = 1\r\n  2\r\nc = 3\r\n").unwrap();

        fixture.edit("a", "b", Some("x"));
        assert_eq!(fixture.content(), "[a]\r\nb = x\r\nc = 3\r\n");

        fixture.edit("a", "d", Some("4"));
        assert_eq!(
            fixture.content(),
            format!("[a]\r\nb = x\r\nc = 3{EOL}d = 4\r\n")
        );

        fixture.edit("a", "c", None);
        assert_eq!(fixture.content(), format!("[a]\r\nb = x{EOL}d = 4\r\n"));
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_target_is_edited() {
        let fixture = Fixture::new("[a]\nb = 1\n");
        let link = fixture.dir.path().join("link");
        std::os::unix::fs::symlink(&fixture.path, &link).unwrap();

        edit_config(&link, "a", "b", Some("2")).unwrap();

        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        fixture.assert_content("[a]\nb = 2\n", "a", "b", Some("2"));
    }

    #[cfg(unix)]
    #[test]
    fn test_file_permissions_are_preserved() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = Fixture::new("[a]\nb = 1\n");
        fs::set_permissions(&fixture.path, fs::Permissions::from_mode(0o600)).unwrap();

        fixture.edit("a", "b", Some("2"));

        let mode = fs::metadata(&fixture.path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
