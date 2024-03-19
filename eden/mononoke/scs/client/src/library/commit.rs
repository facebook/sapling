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
use source_control as thrift;

use crate::args::commit_id::map_commit_ids;
use crate::library::commit_id::render_commit_id;

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
    pub git_extra_headers: Option<BTreeMap<String, String>>,
    pub committer_date: Option<DateTime<FixedOffset>>,
    pub committer: Option<String>,
}

fn timestamp_to_date(timezone: i32, timestamp: i64) -> DateTime<FixedOffset> {
    FixedOffset::east_opt(timezone)
        .unwrap()
        .timestamp_opt(timestamp, 0)
        .unwrap()
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
        let committer = commit.committer.clone();
        // The commit date is recorded as a timestamp plus timezone pair, where
        // the timezone is seconds east of UTC.
        let timestamp = commit.date;
        let timezone = commit.tz;
        let date = timestamp_to_date(timezone, timestamp);
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
        // Only parse the headers which are valid string cause only those will be displayable to the user
        let git_extra_headers = commit.git_extra_headers.as_ref().map(|headers| {
            headers
                .iter()
                .filter_map(|(k, v)| {
                    match (
                        String::from_utf8(k.0.to_vec()),
                        String::from_utf8(v.to_vec()),
                    ) {
                        (Ok(key), Ok(value)) => Some((key, value)),
                        _ => None,
                    }
                })
                .collect()
        });
        let committer_date = commit
            .committer_date
            .map(|timestamp| timestamp_to_date(timezone, timestamp));
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
            git_extra_headers,
            committer_date,
            committer,
        })
    }
}

fn render_date(
    w: &mut dyn Write,
    date: DateTime<FixedOffset>,
    description: &str,
) -> Result<(), Error> {
    let local_date = date.with_timezone(&Local).to_string();
    let date = date.to_string();
    if date != local_date {
        write!(w, "{}: {} ({})\n", description, date, local_date)?;
    } else {
        write!(w, "{}: {}\n", description, date)?;
    }
    Ok(())
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
    render_date(w, commit.date, "Date")?;
    if let Some(committer_date) = commit.committer_date {
        render_date(w, committer_date, "Committer Date")?;
    }
    write!(w, "Author: {}\n", commit.author)?;
    if let Some(committer) = &commit.committer {
        write!(w, "Committer: {}\n", committer)?;
    }
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
    render_date(w, commit.date, "Date")?;
    if let Some(committer_date) = commit.committer_date {
        render_date(w, committer_date, "Committer Date")?;
    }
    write!(w, "Author: {}\n", commit.author)?;
    if let Some(committer) = &commit.committer {
        write!(w, "Committer: {}\n", committer)?;
    }
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
    if let Some(ref headers) = commit.git_extra_headers {
        write!(w, "Git Extra Headers:\n")?;
        for (key, value) in headers {
            write!(w, "    {}={}\n", key, value)?;
        }
    }
    write!(w, "\n{}\n", commit.message)?;
    Ok(())
}
