/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;

use anyhow::Result;
use anyhow::bail;
use anyhow::ensure;
use hgtime::HgTime;
use memchr::memchr;
use minibytes::Text;
use types::SerializationFormat;

/// Normalize " Foo Bar  < a@b.com > " to "Foo Bar <a@b.com>".
/// Reports errors if `name` contains special characters or has unmatched brackets.
///
/// `format` decides which set of rules to apply. Git is a bit stricter:
/// - "test" (no email) is okay as-is for hg, but we need to use "test <>" for git.
/// - "<email>" (no user) is okay as-is for hg, but forbidden by git.
pub(crate) fn normalize_email_user(
    name: &str,
    format: SerializationFormat,
) -> Result<Cow<'_, str>> {
    let name = name.trim();
    ensure!(!name.is_empty(), "invalid name (empty): {:?}", name);

    let invalid_bytes = [b'\0', b'\n', b'\r'];
    ensure!(
        invalid_bytes
            .iter()
            .all(|&b| memchr(b, name.as_bytes()).is_none()),
        "invalid name (special character): {:?}",
        name
    );

    let left_bracket_pos = memchr(b'<', name.as_bytes());
    let right_bracket_pos = memchr(b'>', name.as_bytes());

    let normalized_name = match (left_bracket_pos, right_bracket_pos, format) {
        (None, None, SerializationFormat::Hg) => Cow::Borrowed(name),
        (None, None, SerializationFormat::Git) => Cow::Owned(format!("{} <>", name)),
        (Some(p1), Some(p2), _)
            if p1 < p2
                && memchr(b'<', &name.as_bytes()[p1 + 1..]).is_none()
                && memchr(b'>', &name.as_bytes()[p2 + 1..]).is_none() =>
        {
            ensure!(
                p1 > 0 || matches!(format, SerializationFormat::Hg),
                "invalid name (empty user): {:?}",
                name
            );
            ensure!(
                p2 + 1 == name.len(),
                "invalid name (content after email): {:?}",
                name
            );
            let user = name[..p1].trim();
            let email = name[p1 + 1..p2].trim();
            if user.len() + 1 + email.len() + 2 == name.len() && name[..p1].ends_with(' ') {
                // use `name` as-is
                Cow::Borrowed(name)
            } else if user.is_empty() {
                Cow::Owned(format!("<{}>", email))
            } else {
                Cow::Owned(format!("{} <{}>", user, email))
            }
        }
        _ => bail!("invalid name (mismatched brackets): {:?}", name),
    };

    Ok(normalized_name)
}

/// Write multi-line `message` to `out`. Each line is prefixed by `line_prefix`.
/// `message` is normalized (trimmed trailing spaces, and leading,
/// trailing empty lines, `\r\n` becomes `\n`).
///
/// The last `\n` is not written. The callsite can choose to write it or not.
/// Typically, hg commit message does not end with `\n` but git does.
///
/// Returns `empty`, `true` if nothing was written.
pub(crate) fn write_multi_line(message: &str, line_prefix: &str, out: &mut String) -> Result<bool> {
    // Trim empty lines.
    let message = message.trim_matches(['\r', '\n']);
    // Trim trailing spaces per line.
    let mut empty = true;
    for line in message.lines() {
        if !empty {
            out.push('\n');
        }
        out.push_str(line_prefix);
        let line = line.trim_end_matches(' ');
        out.push_str(line);
        empty = false;
    }
    Ok(empty)
}

pub(crate) trait HgTimeExt {
    fn to_text(&self) -> Text;
}

impl HgTimeExt for HgTime {
    fn to_text(&self) -> Text {
        format!("{} {}", self.unixtime, self.offset).into()
    }
}

/// Produce "title" and indented commit text. Useful for error messages.
pub(crate) fn with_indented_commit_text(title: &str, text: &str) -> String {
    let mut result = title.to_string();
    result.push('\n');
    let _ = write_multi_line(text, "  ", &mut result);
    result
}

#[cfg(test)]
pub(crate) mod tests {
    use hgtime::HgTime;

    use super::*;

    #[test]
    fn test_normalize_email_user() {
        fn normalize_email_user_to_str(name: &str, format: SerializationFormat) -> String {
            match normalize_email_user(name, format) {
                Err(e) => format!("Err({})", e.to_string().split(':').next().unwrap()),
                Ok(Cow::Borrowed(v)) => format!("Borrowed({})", v),
                Ok(Cow::Owned(v)) => format!("Owned({})", v),
            }
        }
        fn t(name: &str) -> String {
            let git = normalize_email_user_to_str(name, SerializationFormat::Git);
            let hg = normalize_email_user_to_str(name, SerializationFormat::Hg);
            if git == hg {
                git
            } else {
                format!("git: {git}; hg: {hg}")
            }
        }

        assert_eq!(t(""), "Err(invalid name (empty))");
        assert_eq!(t(" "), "Err(invalid name (empty))");
        assert_eq!(t("\n"), "Err(invalid name (empty))");
        assert_eq!(t("\0"), "Err(invalid name (special character))");
        assert_eq!(t("a\n <b>"), "Err(invalid name (special character))");
        assert_eq!(t("a <b\0>"), "Err(invalid name (special character))");
        assert_eq!(
            t("foo\0bar <a@b.com>"),
            "Err(invalid name (special character))"
        );

        assert_eq!(t("a"), "git: Owned(a <>); hg: Borrowed(a)");
        assert_eq!(t(" a "), "git: Owned(a <>); hg: Borrowed(a)");
        assert_eq!(
            t(" <a>"),
            "git: Err(invalid name (empty user)); hg: Owned(<a>)"
        );

        assert_eq!(t(" a < > "), "Owned(a <>)");
        assert_eq!(t(" a <> "), "Borrowed(a <>)");
        assert_eq!(t(" a  < a > "), "Owned(a <a>)");
        assert_eq!(t(" a b"), "git: Owned(a b <>); hg: Borrowed(a b)");
        assert_eq!(t("a  <>"), "Owned(a <>)");
        assert_eq!(t("a <>"), "Borrowed(a <>)");
        assert_eq!(t("a <a>"), "Borrowed(a <a>)");
        assert_eq!(t("a <a> "), "Borrowed(a <a>)");
        assert_eq!(t("a < a > "), "Owned(a <a>)");
        assert_eq!(t("a<a>"), "Owned(a <a>)");
        assert_eq!(t("a  <a>"), "Owned(a <a>)");
        assert_eq!(t("a  b  < c  d >"), "Owned(a  b <c  d>)");

        assert_eq!(t("a <<a>"), "Err(invalid name (mismatched brackets))");
        assert_eq!(t("a <<a>>"), "Err(invalid name (mismatched brackets))");
        assert_eq!(t("a <a"), "Err(invalid name (mismatched brackets))");
        assert_eq!(t("a >a<"), "Err(invalid name (mismatched brackets))");
        assert_eq!(t("a <a>>"), "Err(invalid name (mismatched brackets))");
        assert_eq!(t("a a>"), "Err(invalid name (mismatched brackets))");
        assert_eq!(t("a <a>a"), "Err(invalid name (content after email))");
    }

    pub(crate) trait ToTuple {
        fn to_tuple(self) -> (i64, i32);
    }

    impl ToTuple for HgTime {
        fn to_tuple(self) -> (i64, i32) {
            (self.unixtime, self.offset)
        }
    }
}
