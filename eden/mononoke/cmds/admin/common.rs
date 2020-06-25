/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use cloned::cloned;
use cmdlib::helpers;
use context::CoreContext;
use futures::future::TryFutureExt;
use futures_ext::{FutureExt, StreamExt};
use futures_old::{
    future::{self, Future},
    stream::Stream,
};
use manifest::ManifestOps;
use mercurial_types::{HgChangesetId, HgFileNodeId, MPath};
use mononoke_types::{BonsaiChangeset, DateTime, Timestamp};
use serde_json::{json, to_string_pretty};
use slog::{debug, Logger};
use std::collections::HashMap;

pub const LATEST_REPLAYED_REQUEST_KEY: &str = "latest-replayed-request";

pub fn fetch_bonsai_changeset(
    ctx: CoreContext,
    rev: &str,
    repo: &BlobRepo,
) -> impl Future<Item = BonsaiChangeset, Error = Error> {
    helpers::csid_resolve(ctx.clone(), repo.clone(), rev.to_string()).and_then({
        cloned!(ctx, repo);
        move |csid| csid.load(ctx, repo.blobstore()).compat().from_err()
    })
}

pub fn print_bonsai_changeset(bcs: &BonsaiChangeset) {
    println!(
        "BonsaiChangesetId: {} \n\
                     Author: {} \n\
                     Message: {} \n\
                     FileChanges:",
        bcs.get_changeset_id(),
        bcs.author(),
        bcs.message().lines().next().unwrap_or("")
    );

    for (path, file_change) in bcs.file_changes() {
        match file_change {
            Some(file_change) => match file_change.copy_from() {
                Some(_) => println!("\t COPY/MOVE: {} {}", path, file_change.content_id()),
                None => println!("\t ADDED/MODIFIED: {} {}", path, file_change.content_id()),
            },
            None => println!("\t REMOVED: {}", path),
        }
    }
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
    cs_id
        .load(ctx.clone(), repo.blobstore())
        .compat()
        .from_err()
        .map(|cs| cs.manifestid().clone())
        .and_then({
            cloned!(ctx, repo);
            move |root_mf_id| {
                root_mf_id
                    .find_entries(ctx, repo.get_blobstore(), paths.clone())
                    .filter_map(|(path, entry)| Some((path?, entry.into_leaf()?.1)))
                    .collect_to::<HashMap<_, _>>()
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
