// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use cloned::cloned;
use context::CoreContext;
use failure_ext::{err_msg, format_err, Error};
use futures::future::{self, Future};
use futures_ext::FutureExt;
use mercurial_types::{Changeset, HgChangesetId, HgFileNodeId, MPath};
use mononoke_types::{BonsaiChangeset, DateTime, Timestamp};
use serde_json::{json, to_string_pretty};
use slog::{debug, Logger};
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

// The function retrieves the HgFileNodeId of a file, based on path and rev.
// If the path is not valid an error is expected.
pub fn get_file_nodes(
    ctx: CoreContext,
    logger: Logger,
    repo: &BlobRepo,
    cs_id: HgChangesetId,
    paths: Vec<MPath>,
) -> impl Future<Item = Vec<HgFileNodeId>, Error = Error> {
    repo.get_changeset_by_changesetid(ctx.clone(), cs_id)
        .map(|cs| cs.manifestid().clone())
        .and_then({
            cloned!(ctx, repo);
            move |root_mf_id| {
                repo.find_files_in_manifest(ctx, root_mf_id, paths.clone())
                    .map(move |manifest_entries| {
                        let mut existing_hg_nodes = Vec::new();
                        let mut non_existing_paths = Vec::new();

                        for path in paths.iter() {
                            match manifest_entries.get(&path) {
                                Some(hg_node) => existing_hg_nodes.push(*hg_node),
                                None => non_existing_paths.push(path.clone()),
                            };
                        }
                        (non_existing_paths, existing_hg_nodes)
                    })
            }
        })
        .and_then({
            move |(non_existing_paths, existing_hg_nodes)| match non_existing_paths.len() {
                0 => {
                    debug!(logger, "All the file paths are valid");
                    future::ok(existing_hg_nodes).right_future()
                }
                _ => future::err(format_err!(
                    "failed to identify the files associated with the file paths {:?}",
                    non_existing_paths
                ))
                .left_future(),
            }
        })
}
