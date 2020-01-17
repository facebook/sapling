/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use cloned::cloned;
use cmdlib::helpers;
use context::CoreContext;
use futures::future::{self, Future};
use futures_ext::FutureExt;
use mercurial_types::{HgChangesetId, HgFileNodeId, MPath};
use mononoke_types::{BonsaiChangeset, DateTime, Timestamp};
use serde_json::{json, to_string_pretty};
use slog::{debug, Logger};

pub const LATEST_REPLAYED_REQUEST_KEY: &'static str = "latest-replayed-request";

pub fn fetch_bonsai_changeset(
    ctx: CoreContext,
    rev: &str,
    repo: &BlobRepo,
) -> impl Future<Item = BonsaiChangeset, Error = Error> {
    helpers::csid_resolve(ctx.clone(), repo.clone(), rev.to_string()).and_then({
        cloned!(ctx, repo);
        move |bcs_id| repo.get_bonsai_changeset(ctx, bcs_id)
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
