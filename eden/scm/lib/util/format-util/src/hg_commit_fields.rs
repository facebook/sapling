/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::fmt::Write as _;

use anyhow::bail;
use anyhow::Context as _;
use anyhow::Result;
use minibytes::Text;
use once_cell::sync::OnceCell;
use serde::Deserialize;
use storemodel::SerializationFormat;
use types::hgid::NULL_ID;
use types::Id20;
use types::RepoPath;

use crate::normalize_email_user;
use crate::utils::write_multi_line;
pub use crate::CommitFields;
use crate::HgTime;

/// Holds the Hg commit text. Fields can be lazily parsed.
pub struct HgCommitLazyFields {
    text: Text,
    fields: OnceCell<Box<HgCommitFields>>,
}

/// Fields of a hg commit. Enough information to serialize to text.
#[derive(Default, Deserialize)]
#[cfg_attr(test, derive(Clone))]
pub struct HgCommitFields {
    tree: Id20,
    author: Text,
    date: HgTime,
    extras: BTreeMap<Text, Text>,
    files: Vec<Text>,
    message: Text,
}

impl HgCommitFields {
    fn from_text(text: &Text) -> Result<Self> {
        // {tree}
        // {author}
        // {date_seconds} {date timezone} {extra}
        // {files}
        //
        // {message}
        let mut result = Self::default();
        let mut last_pos = 0;

        enum State {
            Tree,
            User,
            TimeExtra,
            Files,
        }
        let mut state = State::Tree;

        for pos in memchr::memchr_iter(b'\n', text.as_bytes()) {
            let line = text.slice(last_pos..pos);
            match state {
                State::Tree => {
                    result.tree = Id20::from_hex(line.as_bytes())?;
                    state = State::User;
                }
                State::User => {
                    result.author = line;
                    state = State::TimeExtra;
                }
                State::TimeExtra => {
                    let (date, maybe_extras) = parse_date(&line)?;
                    result.date = date;
                    if let Some(extras) = maybe_extras {
                        // key:value separated by '\0', with simple escaping rules.
                        for extra in extras.split('\0') {
                            if let Some((key, value)) = extra.split_once(':') {
                                let key = extra_unescape(text.slice_to_bytes(key));
                                let value = extra_unescape(text.slice_to_bytes(value));
                                result.extras.insert(key, value);
                            }
                        }
                    }
                    state = State::Files;
                }
                State::Files => {
                    if line.is_empty() {
                        // The rest is "commit message".
                        result.message = text.slice(pos + 1..);
                        break;
                    } else {
                        result.files.push(line);
                    }
                }
            }
            last_pos = pos + 1;
        }

        Ok(result)
    }

    /// Serialize fields to "text".
    pub fn to_text(&self) -> Result<String> {
        let author = normalize_email_user(&self.author, SerializationFormat::Hg)?;

        let len = Id20::hex_len()
            + self.author.len()
            + self
                .extras
                .iter()
                .map(|(k, v)| k.len() + v.len() + 2usize)
                .sum::<usize>()
            + self.files.iter().map(|p| p.len() + 1).sum::<usize>()
            + self.message.len()
            + 32;
        let mut result = String::with_capacity(len);

        // tree
        result.push_str(&self.tree.to_hex());
        result.push('\n');

        // author
        result.push_str(&author);
        result.push('\n');

        // date, extra
        write!(&mut result, "{}", self.date.unixtime)?;
        result.push(' ');
        write!(&mut result, "{}", self.date.offset)?;
        for (i, (k, v)) in self.extras.iter().enumerate() {
            result.push(if i == 0 { ' ' } else { '\0' });
            result.push_str(&extra_escape(k.clone()));
            result.push(':');
            result.push_str(&extra_escape(v.clone()));
        }
        result.push('\n');

        // files
        for path in &self.files {
            let _ = RepoPath::from_str(path)?;
            result.push_str(path);
            result.push('\n');
        }

        // message
        write_message(&self.message, &mut result)?;

        Ok(result)
    }

    fn committer_and_date(&self) -> Option<(&str, &str)> {
        let committer = self.extras.get("committer")?;
        let committer_date = self.extras.get("committer_date");
        match committer_date {
            Some(committer_date) => {
                // Two fields. "foo bar", "100000 0".
                Some((committer.trim(), committer_date.trim()))
            }
            None => {
                // One field. "foo bar 100000 0". Split from the right side.
                let mut parts = committer.rsplitn(3, ' ');
                let committer_name = parts.nth(2)?;
                let rest = &committer[committer_name.len()..];
                Some((committer_name.trim(), rest.trim()))
            }
        }
    }
}

impl HgCommitLazyFields {
    pub fn new(text: Text) -> Self {
        Self {
            text,
            fields: Default::default(),
        }
    }

    pub fn fields(&self) -> Result<&HgCommitFields> {
        let fields = self
            .fields
            .get_or_try_init(|| HgCommitFields::from_text(&self.text).map(Box::new))?;
        Ok(fields)
    }
}

impl CommitFields for HgCommitLazyFields {
    fn root_tree(&self) -> Result<Id20> {
        if self.text.is_empty() {
            return Ok(NULL_ID);
        }
        if let Some(fields) = self.fields.get() {
            return Ok(fields.tree);
        }
        // Extract tree without parsing all fields.
        if let Some(hex) = self.text.get(..Id20::hex_len()) {
            return Ok(Id20::from_hex(hex.as_bytes())?);
        }
        bail!("invalid hg commit format");
    }

    fn author_name(&self) -> Result<&str> {
        Ok(self.fields()?.author.as_ref())
    }

    fn committer_name(&self) -> Result<Option<&str>> {
        Ok(self.fields()?.committer_and_date().map(|v| v.0))
    }

    fn author_date(&self) -> Result<HgTime> {
        let fields = self.fields()?;
        let date = if let Some(date_str) = fields.extras.get("author_date") {
            parse_date(date_str.as_ref())?.0
        } else {
            fields.date
        };
        Ok(date)
    }

    fn committer_date(&self) -> Result<Option<HgTime>> {
        match self.fields()?.committer_and_date() {
            None => Ok(None),
            Some((_, date_str)) => Ok(Some(parse_date(date_str)?.0)),
        }
    }

    fn files(&self) -> Result<Option<&[Text]>> {
        Ok(Some(&self.fields()?.files))
    }

    fn extras(&self) -> Result<&BTreeMap<Text, Text>> {
        Ok(&self.fields()?.extras)
    }

    fn description(&self) -> Result<&str> {
        Ok(&self.fields()?.message)
    }

    fn format(&self) -> SerializationFormat {
        SerializationFormat::Hg
    }

    fn raw_text(&self) -> &[u8] {
        self.text.as_bytes()
    }
}

/// Returns the `Time` and the rest of `date_str`.
/// date_str is "timestamp tz" used in hg commits
fn parse_date<'a>(date_str: &'a str) -> Result<(HgTime, Option<&'a str>)> {
    let mut parts = date_str.splitn(3, ' ');
    let unixtime: i64 = parts.next().context("missing time")?.parse()?;
    let offset: i32 = parts.next().context("missing tz")?.parse()?;
    Ok((HgTime { unixtime, offset }, parts.next()))
}

fn extra_escape(s: Text) -> Text {
    let special_chars = "\0\n\r\\";
    let need_escape_count = s.chars().filter(|&c| special_chars.contains(c)).count();
    if need_escape_count > 0 {
        let mut result = String::with_capacity(s.len() + need_escape_count);
        for ch in s.chars() {
            match ch {
                '\0' => result.push_str("\\0"),
                '\n' => result.push_str("\\n"),
                '\r' => result.push_str("\\r"),
                '\\' => result.push_str("\\\\"),
                _ => result.push(ch),
            }
        }
        result.into()
    } else {
        s
    }
}

fn extra_unescape(s: Text) -> Text {
    if s.contains('\\') {
        let mut result = String::with_capacity(s.len());
        let mut escaped = false;
        for ch in s.chars() {
            match (escaped, ch) {
                (false, '\\') => {
                    escaped = true;
                }
                (false, _) => result.push(ch),
                (true, _) => {
                    let unescaped_ch = match ch {
                        '0' => '\0',
                        'n' => '\n',
                        'r' => '\r',
                        _ => ch,
                    };
                    result.push(unescaped_ch);
                    escaped = false;
                }
            }
        }
        result.into()
    } else {
        s
    }
}

fn write_message(message: &str, out: &mut String) -> Result<()> {
    // Empty line indicates the start of commit message.
    out.push('\n');
    // For hg, empty commit message is allowed.
    let _empty = write_multi_line(message, "", out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::tests::ToTuple;

    #[test]
    fn test_extra_escape() {
        let s = "abc0nr\\\\文字\\0\\r\\n\\\\\0\r\n\\";
        let escaped = extra_escape(Text::from_static(s));
        let unescaped = extra_unescape(escaped);
        assert_eq!(s, unescaped.as_ref());
    }

    #[test]
    fn test_parse_hg_commit_with_extra_and_files() {
        let text = concat!(
            "98edb6a9c7a48cae7a1ed9a39600952547daaebb\n",
            "Alice 1 <a@example.com>\n",
            "1714100000 25200 committer:Bob \\\\ 2 <b@example.com>\0committer_date:1714200000 -28800\n",
            "a/1.txt\n",
            "b/2.txt\n",
            "\n",
            "This is the commit message.\n",
            "\n",
            "Signed-off-by: Alice <a@example.com>",
        );
        let fields = HgCommitLazyFields::new(text.into());
        assert_eq!(
            fields.root_tree().unwrap().to_hex(),
            "98edb6a9c7a48cae7a1ed9a39600952547daaebb"
        );
        assert_eq!(fields.author_name().unwrap(), "Alice 1 <a@example.com>");
        assert_eq!(
            fields.committer_name().unwrap().unwrap(),
            "Bob \\ 2 <b@example.com>"
        );
        assert_eq!(
            fields.author_date().unwrap().to_tuple(),
            (1714100000, 25200)
        );
        assert_eq!(
            fields.committer_date().unwrap().unwrap().to_tuple(),
            (1714200000, -28800)
        );
        assert_eq!(
            format!("{:?}", fields.extras().unwrap()),
            "{\"committer\": \"Bob \\\\ 2 <b@example.com>\", \"committer_date\": \"1714200000 -28800\"}"
        );
        assert_eq!(
            format!("{:?}", fields.files().unwrap().unwrap()),
            "[\"a/1.txt\", \"b/2.txt\"]"
        );
        assert_eq!(
            fields.description().unwrap(),
            "This is the commit message.\n\nSigned-off-by: Alice <a@example.com>"
        );
        assert_eq!(fields.raw_text(), text.as_bytes());
        assert_eq!(
            fields.root_tree().unwrap().to_hex(),
            "98edb6a9c7a48cae7a1ed9a39600952547daaebb"
        );

        let text2 = fields.fields().unwrap().to_text().unwrap();
        assert_eq!(text2, text);
    }

    #[test]
    fn test_parse_hg_commit_without_extra_and_files() {
        let text = concat!(
            "98edb6a9c7a48cae7a1ed9a39600952547daaebb\n",
            "Alice 1 <a@example.com>\n",
            "1714100000 25200\n",
            "\n",
            "This is the commit message.",
        );
        let fields = HgCommitLazyFields::new(text.into());
        assert_eq!(
            fields.root_tree().unwrap().to_hex(),
            "98edb6a9c7a48cae7a1ed9a39600952547daaebb"
        );
        assert_eq!(fields.author_name().unwrap(), "Alice 1 <a@example.com>");
        assert_eq!(fields.committer_name().unwrap(), None);
        assert_eq!(
            fields.author_date().unwrap().to_tuple(),
            (1714100000, 25200)
        );
        assert_eq!(fields.committer_date().unwrap(), None);
        assert_eq!(format!("{:?}", fields.extras().unwrap()), "{}");
        assert_eq!(format!("{:?}", fields.files().unwrap().unwrap()), "[]");
        assert_eq!(fields.description().unwrap(), "This is the commit message.");
        assert_eq!(fields.raw_text(), text.as_bytes());
        assert_eq!(
            fields.root_tree().unwrap().to_hex(),
            "98edb6a9c7a48cae7a1ed9a39600952547daaebb"
        );

        let text2 = fields.fields().unwrap().to_text().unwrap();
        assert_eq!(text2, text);
    }

    #[test]
    fn test_uncommon_fields() {
        // Use uncommon characters in fields, to test escaping, etc.
        let fields1 = HgCommitFields {
            author: "a\\b".into(),
            files: vec!["f/g h".into()],
            extras: BTreeMap::from_iter([("e1".into(), "foo\0\n".into())]),
            message: "  okay\n  some\0\\thing".into(),
            ..Default::default()
        };

        // should round-trip
        let text1 = fields1.to_text().unwrap();
        let fields2 = HgCommitFields::from_text(&text1.into()).unwrap();
        assert_eq!(&fields1.extras, &fields2.extras);
        assert_eq!(&fields1.message, &fields2.message);

        // should reject bad author name
        let bad_fields = HgCommitFields {
            author: "a\0b".into(),
            ..fields1.clone()
        };
        assert!(bad_fields.to_text().is_err());

        // should reject bad file names
        let bad_fields = HgCommitFields {
            files: vec!["a\n".into()],
            ..fields1.clone()
        };
        assert!(bad_fields.to_text().is_err());
    }

    #[test]
    fn test_committer_name_and_date() {
        let t = |extras: &[(&'static str, &'static str)]| -> String {
            let fields = HgCommitFields {
                author: "a".into(),
                extras: BTreeMap::from_iter(
                    extras
                        .iter()
                        .map(|(k, v)| (Text::from_static(k), Text::from_static(v))),
                ),
                ..Default::default()
            };
            let lazy_fields = HgCommitLazyFields::new(fields.to_text().unwrap().into());
            let name = lazy_fields.committer_name().unwrap();
            let date = lazy_fields.committer_date().unwrap();
            format!(
                "{} {:?}",
                name.unwrap_or("None"),
                date.map(|t| t.to_tuple())
            )
        };

        // "committer" extra contains name and date (used by Mononoke and hg-git)
        assert_eq!(
            t(&[("committer", "foo <a@b> 1600 -10")]),
            "foo <a@b> Some((1600, -10))"
        );
        assert_eq!(t(&[("committer", "foo 1600 -10")]), "foo Some((1600, -10))");
        assert_eq!(
            t(&[("committer", "committer <> 1590978600 0")]),
            "committer <> Some((1590978600, 0))"
        );

        // "committer" and "committer_date" are separate extra fields
        assert_eq!(
            t(&[("committer", "foo <a@b>"), ("committer_date", "100 0")]),
            "foo <a@b> Some((100, 0))"
        );
        assert_eq!(
            t(&[("committer", "f b"), ("committer_date", "100 0")]),
            "f b Some((100, 0))"
        );

        // Committer info is absent
        assert_eq!(t(&[]), "None None");
    }

    #[test]
    fn test_empty_text() {
        let lazy_fields = HgCommitLazyFields::new(Default::default());
        assert!(lazy_fields.root_tree().unwrap().is_null());
        assert!(lazy_fields.fields().unwrap().tree.is_null());
        assert!(lazy_fields.root_tree().unwrap().is_null());
    }
}
