/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::fmt::Write as _;

use anyhow::bail;
use anyhow::ensure;
use anyhow::Context as _;
use anyhow::Result;
use minibytes::Text;
use once_cell::sync::OnceCell;
use serde::Deserialize;
use storemodel::SerializationFormat;
use types::Id20;

use crate::normalize_email_user;
pub use crate::CommitFields;

/// Holds the Git commit text. Fields can be lazily fields.
pub struct GitCommitLazyFields {
    text: Text,
    fields: OnceCell<Box<GitCommitFields>>,
}

/// Fields of a git commit. Enough information to serialize to text.
#[derive(Default, Deserialize)]
pub struct GitCommitFields {
    tree: Id20,
    parents: Vec<Id20>,
    author: Text,
    date: Date,
    committer: Text,
    committer_date: Date,
    message: Text,
    // e.g. "gpgsig", "gpgsig-sha256", "mergetag".
    // See https://git-scm.com/docs/signature-format
    #[serde(default)]
    extras: BTreeMap<Text, Text>,
}

type Date = (u64, i32);

impl GitCommitFields {
    fn from_text(text: &Text) -> Result<Self> {
        // tree {tree_sha}
        // {parents}
        // author {author_name} <{author_email}> {author_date_seconds} {author_date_timezone}
        // committer {committer_name} <{committer_email}> {committer_date_seconds} {committer_date_timezone}
        // {gpgsig ...
        //  ...
        //  ...}
        //
        // {commit message}
        let mut result = Self::default();
        let mut last_pos = 0;
        let mut current_extra: Option<(Text, String)> = None;
        for pos in memchr::memchr_iter(b'\n', text.as_bytes()) {
            let line = &text[last_pos..pos];
            if let Some((name, mut value)) = current_extra {
                if let Some(cont) = line.strip_prefix(' ') {
                    // line is part of a multi-line extra.
                    value.push('\n');
                    value.push_str(cont);
                    current_extra = Some((name, value));
                    last_pos = pos + 1;
                    continue;
                } else {
                    // line does not belong to this extra.
                    result.extras.insert(name.clone(), value.into());
                    current_extra = None;
                }
            }

            if let Some(hex) = line.strip_prefix("tree ") {
                result.tree = Id20::from_hex(hex.as_bytes())?;
            } else if let Some(hex) = line.strip_prefix("parent ") {
                result.parents.push(Id20::from_hex(hex.as_bytes())?);
            } else if let Some(line) = line.strip_prefix("author ") {
                (result.author, result.date) = parse_name_date(text.slice_to_bytes(line))?;
            } else if let Some(line) = line.strip_prefix("committer ") {
                (result.committer, result.committer_date) =
                    parse_name_date(text.slice_to_bytes(line))?;
            } else if let (Some((name, value)), None) = (line.split_once(' '), &current_extra) {
                current_extra = Some((text.slice_to_bytes(name), value.to_string()));
            } else if line.is_empty() {
                // Treat the rest as "message".
                result.message = text.slice(pos + 1..);
                // "message" is the last part.
                break;
            } else {
                ensure!(
                    !result.committer.is_empty(),
                    "bogus line in git commit: {}",
                    line
                );
            }
            last_pos = pos + 1;
        }
        Ok(result)
    }

    /// Serialize fields to "text".
    pub fn to_text(&self) -> Result<String> {
        ensure!(!self.message.is_empty(), "message cannot be empty");

        let len = (1 + self.parents.len()) * (8 + Id20::hex_len())
            + self.author.len()
            + self.committer.len()
            + self.message.len()
            + 64;
        let mut result = String::with_capacity(len);

        // tree
        result.push_str("tree ");
        result.push_str(&self.tree.to_hex());
        result.push('\n');

        // parents
        for p in &self.parents {
            result.push_str("parent ");
            result.push_str(&p.to_hex());
            result.push('\n');
        }

        // author, committer
        write_name_date("author", &self.author, self.date, &mut result)?;
        write_name_date(
            "committer",
            &self.committer,
            self.committer_date,
            &mut result,
        )?;

        // extra (e.g. gpgsig)
        for (name, value) in &self.extras {
            write_extra(name, value, &mut result)?;
        }

        // message
        result.push('\n');
        result.push_str(self.message.trim_matches('\n'));
        result.push('\n');

        Ok(result)
    }
}

impl GitCommitLazyFields {
    pub fn new(text: Text) -> Self {
        Self {
            text,
            fields: Default::default(),
        }
    }

    pub fn fields(&self) -> Result<&GitCommitFields> {
        let fields = self
            .fields
            .get_or_try_init(|| GitCommitFields::from_text(&self.text).map(Box::new))?;
        Ok(fields)
    }
}

impl CommitFields for GitCommitLazyFields {
    fn root_tree(&self) -> Result<Id20> {
        if let Some(fields) = self.fields.get() {
            return Ok(fields.tree);
        }
        // Extract tree without parsing all fields.
        if let Some(rest) = self.text.strip_prefix("tree ") {
            if let Some(hex) = rest.get(..Id20::hex_len()) {
                return Ok(Id20::from_hex(hex.as_bytes())?);
            }
        }
        bail!("invalid git commit format: {}", &self.text);
    }

    fn author_name(&self) -> Result<&str> {
        Ok(self.fields()?.author.as_ref())
    }

    fn committer_name(&self) -> Result<Option<&str>> {
        Ok(Some(self.fields()?.committer.as_ref()))
    }

    fn author_date(&self) -> Result<(u64, i32)> {
        Ok(self.fields()?.date)
    }

    fn committer_date(&self) -> Result<Option<(u64, i32)>> {
        Ok(Some(self.fields()?.committer_date))
    }

    fn extras(&self) -> Result<Option<&BTreeMap<Text, Text>>> {
        let extras = &self.fields()?.extras;
        if extras.is_empty() {
            Ok(None)
        } else {
            Ok(Some(extras))
        }
    }

    fn parents(&self) -> Result<Option<&[Id20]>> {
        Ok(Some(&self.fields()?.parents))
    }

    fn description(&self) -> Result<&str> {
        Ok(&self.fields()?.message)
    }

    fn format(&self) -> SerializationFormat {
        SerializationFormat::Git
    }

    fn raw_text(&self) -> &[u8] {
        self.text.as_bytes()
    }
}

fn parse_name_date(line: Text) -> Result<(Text, Date)> {
    // {name} <{email}> {date_seconds} {date_timezone}
    let mut parts = line.rsplitn(3, ' ');
    let tz_seconds = {
        // +HHMM or -HHMM
        let tz_str = parts.next().context("missing timezone")?;
        ensure!(tz_str.len() == 5, "invalid git timezone: {}", tz_str);
        // Git's "-0700" = Hg's "25200"
        let sign = if tz_str.starts_with('-') { 1 } else { -1 };
        let hours = tz_str[1..3].parse::<i32>()?;
        let minutes = tz_str[3..5].parse::<i32>()?;
        (hours * 3600 + minutes * 60) * sign
    };
    let date_seconds = {
        let date_str = parts.next().context("missing date")?;
        date_str.parse::<u64>()?
    };
    let name = {
        let name_str = parts.next().context("missing name")?;
        line.slice_to_bytes(name_str)
    };
    Ok((name, (date_seconds, tz_seconds)))
}

fn write_extra(name: &str, value: &str, out: &mut String) -> Result<()> {
    if name == "committer" || name == "committer_date" || name == "branch" {
        // "committer" was written before; "branch" is useless - just skip it.
        return Ok(());
    }
    let bad_extra_names = ["author", "parent", "tree"];
    ensure!(
        !name.contains("\n") && !name.contains(" ") && bad_extra_names.iter().all(|&n| n != name),
        "invalid extra name"
    );
    out.push_str(name);
    for line in value.trim_end_matches('\n').split('\n') {
        out.push(' ');
        out.push_str(line);
        out.push('\n');
    }
    Ok(())
}

fn write_name_date(prefix: &str, name: &str, date: Date, out: &mut String) -> Result<()> {
    let name = normalize_email_user(name, SerializationFormat::Git)?;
    out.push_str(prefix);
    out.push(' ');
    out.push_str(&name);
    out.push(' ');
    write!(out, "{}", date.0)?;
    out.push(' ');
    write_git_tz(date.1, out)?;
    out.push('\n');
    Ok(())
}

fn write_git_tz(tz_seconds: i32, out: &mut String) -> Result<()> {
    let sign = if tz_seconds <= 0 { '+' } else { '-' };
    out.push(sign);
    let hh = tz_seconds.abs() / 3600;
    write!(out, "{:02}", hh)?;
    let mm = (tz_seconds.abs() % 3600) / 60;
    write!(out, "{:02}", mm)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_git_commit_basic() {
        let text = r#"tree 98edb6a9c7a48cae7a1ed9a39600952547daaebb
parent 8e1b0fe96dc24617d192af955208ddd485888bd6
parent 7769e9429c9c3563110d296e745b8e45581bbe1f
author Alice 1 <a@example.com> 1714100000 -0700
committer Bob 2 <b@example.com> 1714200000 +0800

This is the commit message.

Signed-off-by: Alice <a@example.com>
"#;
        let fields = GitCommitLazyFields::new(text.into());
        assert_eq!(
            fields.root_tree().unwrap().to_hex(),
            "98edb6a9c7a48cae7a1ed9a39600952547daaebb"
        );
        assert_eq!(fields.author_name().unwrap(), "Alice 1 <a@example.com>");
        assert_eq!(
            fields.committer_name().unwrap().unwrap(),
            "Bob 2 <b@example.com>"
        );
        assert_eq!(fields.author_date().unwrap(), (1714100000, 25200));
        assert_eq!(
            fields.committer_date().unwrap().unwrap(),
            (1714200000, -28800)
        );
        assert_eq!(
            format!("{:?}", fields.parents().unwrap().unwrap()),
            "[HgId(\"8e1b0fe96dc24617d192af955208ddd485888bd6\"), HgId(\"7769e9429c9c3563110d296e745b8e45581bbe1f\")]"
        );
        assert_eq!(
            fields.description().unwrap(),
            "This is the commit message.\n\nSigned-off-by: Alice <a@example.com>\n"
        );
        assert_eq!(fields.raw_text(), text.as_bytes());
        assert_eq!(
            fields.root_tree().unwrap().to_hex(),
            "98edb6a9c7a48cae7a1ed9a39600952547daaebb"
        );
        assert_eq!(format!("{:?}", fields.extras().unwrap()), "None");

        let text2 = fields.fields().unwrap().to_text().unwrap();
        assert_eq!(text2, text);
    }

    #[test]
    fn test_parse_git_commit_with_gpgsig() {
        let text = r#"tree 98edb6a9c7a48cae7a1ed9a39600952547daaebb
author Alice <a@example.com> 1714300000 -0001
committer Bob <b@example.com> 1714400000 +0000
data1 foo
 bar
data2 foo bar
gpgsig -- BEGIN --
 
 signature foo bar
 
 -- END --

This is the commit message.
"#;
        let fields = GitCommitLazyFields::new(text.into());
        assert_eq!(fields.author_date().unwrap(), (1714300000, 60));
        assert_eq!(fields.committer_date().unwrap().unwrap(), (1714400000, 0));

        assert_eq!(
            format!("{:?}", fields.extras().unwrap().unwrap()),
            r#"{"data1": "foo\nbar", "data2": "foo bar", "gpgsig": "-- BEGIN --\n\nsignature foo bar\n\n-- END --"}"#
        );
        assert_eq!(
            fields.description().unwrap(),
            "This is the commit message.\n"
        );

        let text2 = fields.fields().unwrap().to_text().unwrap();
        assert_eq!(text2, text);
    }
}
