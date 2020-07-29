/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::lfs_verifier::LfsVerifier;
use anyhow::{bail, Error};
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bookmarks::{BookmarkName, BookmarkUpdateLogEntry};
use bytes::Bytes;
use bytes_old::Bytes as BytesOld;
use cloned::cloned;
use context::CoreContext;
use futures::future::{FutureExt as NewFutureExt, TryFutureExt};
use futures_ext::{try_boxfuture, FutureExt, StreamExt};
use futures_old::{
    future::{self, IntoFuture},
    stream, Future, Stream,
};
use getbundle_response::{
    create_filenodes, create_manifest_entries_stream, get_manifests_and_filenodes,
    PreparedFilenodeEntry, SessionLfsParams,
};
use maplit::hashmap;
use mercurial_bundles::{
    capabilities::{encode_capabilities, Capabilities},
    changegroup::CgVersion,
    create_bundle_stream, parts,
};
use mercurial_revlog::RevlogChangeset;
use mercurial_types::{HgBlobNode, HgChangesetId, MPath};
use mononoke_types::{datetime::Timestamp, hash::Sha256, ChangesetId};
use reachabilityindex::LeastCommonAncestorsHint;
use revset::DifferenceOfUnionsOfAncestorsNodeStream;
use slog::debug;
use std::{collections::HashMap, sync::Arc};

pub fn create_bundle(
    ctx: CoreContext,
    repo: BlobRepo,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    bookmark: BookmarkName,
    bookmark_change: BookmarkChange,
    hg_server_heads: Vec<ChangesetId>,
    lfs_params: SessionLfsParams,
    filenode_verifier: FilenodeVerifier,
) -> impl Future<Item = (BytesOld, HashMap<HgChangesetId, Timestamp>), Error = Error> {
    let commits_to_push = find_commits_to_push(
        ctx.clone(),
        repo.clone(),
        lca_hint.clone(),
        // Always add "from" bookmark, because is must to be on the hg server
        // If it's not then the push will fail anyway
        hg_server_heads
            .into_iter()
            .chain(bookmark_change.get_from().into_iter()),
        bookmark_change.get_to(),
    )
    .collect()
    .map(|reversed| reversed.into_iter().rev().collect());

    commits_to_push
        .and_then({
            move |commits_to_push: Vec<_>| {
                debug!(
                    ctx.logger(),
                    "generating a bundle with {} commits",
                    commits_to_push.len()
                );
                let bundle = create_bundle_impl(
                    ctx.clone(),
                    repo.clone(),
                    bookmark,
                    bookmark_change,
                    commits_to_push.clone(),
                    lfs_params,
                    filenode_verifier,
                );
                let timestamps = fetch_timestamps(ctx, repo, commits_to_push);
                bundle.join(timestamps)
            }
        })
        .boxify()
}

pub enum BookmarkChange {
    Created(ChangesetId),
    Deleted(ChangesetId),
    Moved { from: ChangesetId, to: ChangesetId },
}

impl BookmarkChange {
    pub fn new(entry: &BookmarkUpdateLogEntry) -> Result<Self, Error> {
        match (entry.from_changeset_id, entry.to_changeset_id) {
            (Some(ref from), None) => Ok(BookmarkChange::Deleted(*from)),
            (None, Some(ref to)) => Ok(BookmarkChange::Created(*to)),
            (Some(ref from), Some(ref to)) => Ok(BookmarkChange::Moved {
                from: *from,
                to: *to,
            }),
            (None, None) => bail!("unsupported bookmark move: deletion of non-existent bookmark?",),
        }
    }

    fn get_from(&self) -> Option<ChangesetId> {
        use BookmarkChange::*;

        match self {
            Created(_) => None,
            Deleted(cs_id) => Some(*cs_id),
            Moved { from, .. } => Some(*from),
        }
    }

    fn get_from_hg(
        &self,
        ctx: CoreContext,
        repo: &BlobRepo,
    ) -> impl Future<Item = Option<HgChangesetId>, Error = Error> {
        Self::maybe_get_hg(ctx, self.get_from(), repo)
    }

    fn get_to(&self) -> Option<ChangesetId> {
        use BookmarkChange::*;

        match self {
            Created(cs_id) => Some(*cs_id),
            Deleted(_) => None,
            Moved { to, .. } => Some(*to),
        }
    }

    fn get_to_hg(
        &self,
        ctx: CoreContext,
        repo: &BlobRepo,
    ) -> impl Future<Item = Option<HgChangesetId>, Error = Error> {
        Self::maybe_get_hg(ctx, self.get_to(), repo)
    }

    fn maybe_get_hg(
        ctx: CoreContext,
        maybe_cs: Option<ChangesetId>,
        repo: &BlobRepo,
    ) -> impl Future<Item = Option<HgChangesetId>, Error = Error> {
        match maybe_cs {
            Some(cs_id) => repo
                .get_hg_from_bonsai_changeset(ctx, cs_id)
                .map(Some)
                .left_future(),
            None => future::ok(None).right_future(),
        }
    }
}

#[derive(Clone)]
pub enum FilenodeVerifier {
    NoopVerifier,
    LfsVerifier(LfsVerifier),
}

impl FilenodeVerifier {
    fn verify_entries(
        &self,
        filenode_entries: &HashMap<MPath, Vec<PreparedFilenodeEntry>>,
    ) -> impl Future<Item = (), Error = Error> + 'static {
        match self {
            Self::NoopVerifier => Ok(()).into_future().left_future(),
            Self::LfsVerifier(lfs_verifier) => {
                let lfs_blobs: Vec<(Sha256, u64)> = filenode_entries
                    .values()
                    .map(|entries| entries.into_iter())
                    .flatten()
                    .filter_map(|entry| {
                        entry
                            .maybe_get_lfs_pointer()
                            .map(|(sha256, size)| (sha256, size))
                    })
                    .collect();

                lfs_verifier.verify_lfs_presence(&lfs_blobs).right_future()
            }
        }
    }
}

fn create_bundle_impl(
    ctx: CoreContext,
    repo: BlobRepo,
    bookmark: BookmarkName,
    bookmark_change: BookmarkChange,
    commits_to_push: Vec<HgChangesetId>,
    session_lfs_params: SessionLfsParams,
    filenode_verifier: FilenodeVerifier,
) -> impl Future<Item = BytesOld, Error = Error> {
    let changelog_entries = stream::iter_ok(commits_to_push.clone())
        .map({
            cloned!(ctx, repo);
            move |hg_cs_id| {
                hg_cs_id
                    .load(ctx.clone(), repo.blobstore())
                    .compat()
                    .from_err()
                    .map(move |cs| (hg_cs_id, cs))
            }
        })
        .buffered(100)
        .and_then(|(hg_cs_id, cs)| {
            let revlogcs = RevlogChangeset::new_from_parts(
                cs.parents().clone(),
                cs.manifestid().clone(),
                cs.user().into(),
                cs.time().clone(),
                cs.extra().clone(),
                cs.files().into(),
                cs.message().into(),
            );

            let mut v = Vec::new();
            mercurial_revlog::changeset::serialize_cs(&revlogcs, &mut v)?;
            Ok((
                hg_cs_id.into_nodehash(),
                HgBlobNode::new(Bytes::from(v), revlogcs.p1(), revlogcs.p2()),
            ))
        });

    let entries = {
        cloned!(ctx, repo, commits_to_push, session_lfs_params);
        async move {
            get_manifests_and_filenodes(&ctx, &repo, commits_to_push, &session_lfs_params).await
        }
        .boxed()
        .compat()
    };

    (
        entries,
        bookmark_change.get_from_hg(ctx.clone(), &repo),
        bookmark_change.get_to_hg(ctx.clone(), &repo),
    )
        .into_future()
        .and_then(
            move |((manifests, prepared_filenode_entries), maybe_from, maybe_to)| {
                let mut bundle2_parts =
                    vec![try_boxfuture!(parts::replycaps_part(create_capabilities()))];

                debug!(
                    ctx.logger(),
                    "prepared {} manifests and {} filenodes",
                    manifests.len(),
                    prepared_filenode_entries.len()
                );
                let cg_version = if session_lfs_params.threshold.is_some() {
                    CgVersion::Cg3Version
                } else {
                    CgVersion::Cg2Version
                };

                // Check that the filenodes pass the verifier prior to serializing them.
                let verify_ok = filenode_verifier.verify_entries(&prepared_filenode_entries);

                let filenode_entries =
                    create_filenodes(ctx.clone(), repo.clone(), prepared_filenode_entries).boxify();

                let filenode_entries = verify_ok
                    .and_then(move |_| Ok(filenode_entries))
                    .flatten_stream()
                    .boxify();

                if commits_to_push.len() > 0 {
                    bundle2_parts.push(try_boxfuture!(parts::changegroup_part(
                        changelog_entries,
                        Some(filenode_entries),
                        cg_version,
                    )));

                    bundle2_parts.push(try_boxfuture!(parts::treepack_part(
                        create_manifest_entries_stream(ctx, repo.get_blobstore(), manifests)
                    )));
                }

                bundle2_parts.push(try_boxfuture!(parts::bookmark_pushkey_part(
                    bookmark.to_string(),
                    format!(
                        "{}",
                        maybe_from.map(|x| x.to_string()).unwrap_or(String::new())
                    ),
                    format!(
                        "{}",
                        maybe_to.map(|x| x.to_string()).unwrap_or(String::new())
                    ),
                )));

                let compression = None;
                create_bundle_stream(bundle2_parts, compression)
                    .concat2()
                    .boxify()
            },
        )
}

fn fetch_timestamps(
    ctx: CoreContext,
    repo: BlobRepo,
    hg_cs_ids: impl IntoIterator<Item = HgChangesetId>,
) -> impl Future<Item = HashMap<HgChangesetId, Timestamp>, Error = Error> {
    stream::iter_ok(hg_cs_ids)
        .map(move |hg_cs_id| {
            hg_cs_id
                .load(ctx.clone(), repo.blobstore())
                .compat()
                .from_err()
                .map(move |hg_blob_cs| (hg_cs_id, hg_blob_cs.time().clone().into()))
        })
        .buffered(100)
        .collect_to()
}

fn find_commits_to_push(
    ctx: CoreContext,
    repo: BlobRepo,
    lca_hint_index: Arc<dyn LeastCommonAncestorsHint>,
    hg_server_heads: impl IntoIterator<Item = ChangesetId>,
    maybe_to_cs_id: Option<ChangesetId>,
) -> impl Stream<Item = HgChangesetId, Error = Error> {
    DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
        ctx.clone(),
        &repo.get_changeset_fetcher(),
        lca_hint_index,
        maybe_to_cs_id.into_iter().collect(),
        hg_server_heads.into_iter().collect(),
    )
    .map(move |bcs_id| repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id))
    .buffered(100)
}

// TODO(stash): this should generate different capabilities depending on whether client
// supports changegroup3 or not
fn create_capabilities() -> BytesOld {
    // List of capabilities that was copied from real bundle generated by Mercurial client.
    let caps_ref = hashmap! {
        "HG20" => vec![],
        "b2x:infinitepush" => vec![],
        "b2x:infinitepushmutation" => vec![],
        "b2x:infinitepushscratchbookmarks" => vec![],
        "b2x:rebase" => vec![],
        "bookmarks" => vec![],
        "changegroup" => vec!["01", "02"],
        "digests" => vec!["md5", "sha1", "sha512"],
        "error" => vec!["abort", "unsupportedcntent", "pushraced", "pushkey"],
        "hgtagsfnodes" => vec![],
        "listkeys" => vec![],
        "phases" => vec!["heads"],
        "pushback" => vec![],
        "pushkey" => vec![],
        "remote-changegroup" => vec!["http", "https"],
        "remotefilelog" => vec!["True"],
        "treemanifest" => vec!["True"],
        "treeonly" => vec!["True"],
    };

    let mut caps = hashmap! {};
    for (key, values) in caps_ref {
        let values = values.into_iter().map(|v| v.to_string()).collect();
        caps.insert(key.to_string(), values);
    }

    encode_capabilities(Capabilities::new(caps))
}
