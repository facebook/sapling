/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Helper library for rendering bookmark info

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::io::Write;

use anyhow::Error;
use chrono::FixedOffset;
use chrono::Local;
use chrono::TimeZone;
use serde::Serialize;
use source_control::types as thrift;

use crate::args::commit_id::map_commit_ids;
use crate::lib::commit_id::render_commit_id;

#[derive(Serialize)]
pub(crate) struct BookmarkInfo {
    pub r#type: String, // For JSON output, always "bookmark".
    pub warm_ids: BTreeMap<String, String>,
    pub fresh_ids: BTreeMap<String, String>,
    pub last_update_timestamp_ns: i64,
}

impl TryFrom<&thrift::BookmarkInfo> for BookmarkInfo {
    type Error = Error;

    fn try_from(bookmark: &thrift::BookmarkInfo) -> Result<BookmarkInfo, Error> {
        let warm_ids = map_commit_ids(bookmark.warm_ids.values());
        let fresh_ids = map_commit_ids(bookmark.fresh_ids.values());
        let last_update_timestamp_ns = bookmark.last_update_timestamp_ns;

        Ok(BookmarkInfo {
            r#type: "bookmark".to_string(),
            warm_ids,
            fresh_ids,
            last_update_timestamp_ns,
        })
    }
}

pub(crate) fn render_bookmark_info(
    bookmark_info: &BookmarkInfo,
    requested: &str,
    schemes: &HashSet<String>,
    w: &mut dyn Write,
) -> Result<(), Error> {
    render_commit_id(
        Some(("Warm value", "    ")),
        "\n",
        requested,
        &bookmark_info.warm_ids,
        schemes,
        w,
    )?;
    write!(w, "\n")?;
    render_commit_id(
        Some(("Fresh value", "    ")),
        "\n",
        requested,
        &bookmark_info.fresh_ids,
        schemes,
        w,
    )?;
    write!(w, "\n")?;
    let date = FixedOffset::west(0).timestamp_nanos(bookmark_info.last_update_timestamp_ns);
    let date_str = date.to_string();
    let local_date_str = date.with_timezone(&Local).to_string();
    if date_str != local_date_str {
        write!(w, "Last update: {} ({})\n", date_str, local_date_str)?;
    } else {
        write!(w, "Last update: {}\n", date_str)?;
    }
    Ok(())
}
