/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Helper library for rendering commit info

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::io::Write;

use anyhow::Error;
use chrono::DateTime;
use chrono::FixedOffset;
use chrono::Local;
use chrono::TimeZone;
use serde::Serialize;
use source_control::types as thrift;

use crate::args::commit_id::map_commit_ids;
use crate::lib::commit_id::render_commit_id;

#[derive(Serialize)]
pub(crate) struct CommitInfo {
    pub r#type: String, // For JSON output, always "commit".
    pub ids: BTreeMap<String, String>,
    pub parents: Vec<BTreeMap<String, String>>,
    pub message: String,
    pub date: DateTime<FixedOffset>,
    pub timestamp: i64,
    pub timezone: i32,
    pub author: String,
    pub generation: i64,
    pub extra: BTreeMap<String, String>,
    pub extra_hex: BTreeMap<String, String>,
}

impl TryFrom<&thrift::CommitInfo> for CommitInfo {
    type Error = Error;

    fn try_from(commit: &thrift::CommitInfo) -> Result<CommitInfo, Error> {
        let ids = map_commit_ids(commit.ids.values());
        let parents = commit
            .parents
            .iter()
            .map(|ids| map_commit_ids(ids.values()))
            .collect();
        let message = commit.message.clone();
        let author = commit.author.clone();
        // The commit date is recorded as a timestamp plus timezone pair, where
        // the timezone is seconds east of UTC.
        let timestamp = commit.date;
        let timezone = commit.tz;
        let date = FixedOffset::east(timezone).timestamp(timestamp, 0);
        // Extras are binary data, but usually we want to render them as
        // strings. In the case that they are not UTF-8 strings, they're
        // probably a commit hash, so we should hex-encode them. Record extras
        // that are valid UTF-8 as strings, and hex encode the rest.
        let mut extra = BTreeMap::new();
        let mut extra_hex = BTreeMap::new();
        for (name, value) in commit.extra.iter() {
            match std::str::from_utf8(value) {
                Ok(value) => extra.insert(name.clone(), value.to_string()),
                Err(_) => extra_hex.insert(name.clone(), faster_hex::hex_string(value)),
            };
        }
        Ok(CommitInfo {
            r#type: "commit".to_string(),
            ids,
            parents,
            message,
            date,
            timestamp,
            timezone,
            author,
            generation: commit.generation,
            extra,
            extra_hex,
        })
    }
}

#[allow(dead_code)]
pub(crate) fn render_commit_summary(
    commit: &CommitInfo,
    requested: &str,
    schemes: &HashSet<String>,
    w: &mut dyn Write,
) -> Result<(), Error> {
    render_commit_id(
        Some(("Commit", "    ")),
        "\n",
        requested,
        &commit.ids,
        schemes,
        w,
    )?;
    write!(w, "\n")?;
    let date = commit.date.to_string();
    let local_date = commit.date.with_timezone(&Local).to_string();
    if date != local_date {
        write!(w, "Date: {} ({})\n", date, local_date)?;
    } else {
        write!(w, "Date: {}\n", date)?;
    }
    write!(w, "Author: {}\n", commit.author)?;
    write!(
        w,
        "Summary: {}\n",
        commit.message.lines().next().unwrap_or("")
    )?;
    Ok(())
}

pub(crate) fn render_commit_info(
    commit: &CommitInfo,
    requested: &str,
    schemes: &HashSet<String>,
    w: &mut dyn Write,
) -> Result<(), Error> {
    render_commit_id(
        Some(("Commit", "    ")),
        "\n",
        requested,
        &commit.ids,
        schemes,
        w,
    )?;
    write!(w, "\n")?;
    for (i, parent) in commit.parents.iter().enumerate() {
        let header = if commit.parents.len() == 1 {
            "Parent".to_string()
        } else {
            format!("Parent-{}", i)
        };
        render_commit_id(
            Some((&header, "    ")),
            "\n",
            &format!("Parent {} of {}", i, requested),
            parent,
            schemes,
            w,
        )?;
        write!(w, "\n")?;
    }
    let date = commit.date.to_string();
    let local_date = commit.date.with_timezone(&Local).to_string();
    if date != local_date {
        write!(w, "Date: {} ({})\n", date, local_date)?;
    } else {
        write!(w, "Date: {}\n", date)?;
    }
    write!(w, "Author: {}\n", commit.author)?;
    write!(w, "Generation: {}\n", commit.generation)?;
    if !commit.extra.is_empty() {
        write!(w, "Extra:\n")?;
        for (name, value) in commit.extra.iter() {
            write!(w, "    {}={}\n", name, value)?;
        }
    }
    if !commit.extra_hex.is_empty() {
        write!(w, "Extra-Binary:\n")?;
        for (name, value) in commit.extra_hex.iter() {
            write!(w, "    {}={}\n", name, value)?;
        }
    }
    write!(w, "\n{}\n", commit.message)?;
    Ok(())
}
