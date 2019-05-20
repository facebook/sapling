// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use cloned::cloned;
use context::CoreContext;
use failure_ext::{err_msg, Error};
use futures::future::Future;
use mercurial_types::HgChangesetId;
use mononoke_types::{BonsaiChangeset, DateTime, Timestamp};
use serde_json::{json, to_string_pretty};
use std::str::FromStr;

pub const LATEST_REPLAYED_REQUEST_KEY: &'static str = "latest-replayed-request";

pub fn fetch_bonsai_changeset(
    ctx: CoreContext,
    rev: &str,
    repo: &BlobRepo,
) -> impl Future<Item = BonsaiChangeset, Error = Error> {
    let hg_changeset_id = resolve_hg_rev(ctx.clone(), repo, rev);

    hg_changeset_id
        .and_then({
            cloned!(ctx, repo);
            move |hg_cs| repo.get_bonsai_from_hg(ctx, hg_cs)
        })
        .and_then({
            let rev = rev.to_string();
            move |maybe_bonsai| maybe_bonsai.ok_or(err_msg(format!("bonsai not found for {}", rev)))
        })
        .and_then({
            cloned!(ctx, repo);
            move |bonsai| repo.get_bonsai_changeset(ctx, bonsai)
        })
}

pub fn format_bookmark_log_entry(
    json_flag: bool,
    changeset_id: String,
    reason: BookmarkUpdateReason,
    timestamp: Timestamp,
    changeset_type: &str,
    bookmark: BookmarkName,
    bundle_id: Option<i64>,
) -> String {
    let reason_str = reason.to_string();
    if json_flag {
        let answer = json!({
            "changeset_type": changeset_type,
            "changeset_id": changeset_id,
            "reason": reason_str,
            "timestamp_sec": timestamp.timestamp_seconds(),
            "bundle_id": bundle_id,
        });
        to_string_pretty(&answer).unwrap()
    } else {
        let dt: DateTime = timestamp.into();
        let dts = dt.as_chrono().format("%b %e %T %Y");
        match bundle_id {
            Some(bundle_id) => format!(
                "{} ({}) {} {} {}",
                bundle_id, bookmark, changeset_id, reason, dts
            ),
            None => format!("({}) {} {} {}", bookmark, changeset_id, reason, dts),
        }
    }
}

pub fn resolve_hg_rev(
    ctx: CoreContext,
    repo: &BlobRepo,
    rev: &str,
) -> impl Future<Item = HgChangesetId, Error = Error> {
    let book = BookmarkName::new(&rev).unwrap();
    let hash = HgChangesetId::from_str(rev);

    repo.get_bookmark(ctx, &book).and_then({
        move |r| match r {
            Some(cs) => Ok(cs),
            None => hash,
        }
    })
}
