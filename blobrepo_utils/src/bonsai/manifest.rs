// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// NOTE: This isn't in `bonsai_utils` because blobrepo depends on it, while this depends on
// blobrepo.

use crate::changeset::{visit_changesets, ChangesetVisitMeta, ChangesetVisitor};
use crate::errors::*;
use blobrepo::derive_hg_manifest::derive_hg_manifest;
use blobrepo::internal::IncompleteFilenodes;
use blobrepo::{BlobManifest, BlobRepo, HgBlobChangeset, HgBlobEntry};
use bonsai_utils::{bonsai_diff, BonsaiDiffResult};
use cloned::cloned;
use context::CoreContext;
use failure_ext::bail_msg;
use futures::{
    future::{self, Either},
    Future, Stream,
};
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use mercurial_types::manifest_utils::{changed_entry_stream, ChangedEntry};
use mercurial_types::{Changeset, HgChangesetId, HgEntry, HgManifestId, HgNodeHash, Type};
use mononoke_types::DateTime;
use slog::{debug, Logger};
use std::{collections::HashSet, fmt, sync::Arc};

#[derive(Clone, Debug)]
pub enum BonsaiMFVerifyResult {
    Valid {
        lookup_mf_id: HgNodeHash,
        computed_mf_id: HgNodeHash,
    },
    // ValidDifferentHash means that the root manifest ID didn't match up, but that that was
    // because of an expected difference in hash that isn't substantive.
    ValidDifferentId(BonsaiMFVerifyDifference),
    Invalid(BonsaiMFVerifyDifference),
    Ignored(HgChangesetId),
}

impl BonsaiMFVerifyResult {
    pub fn is_valid(&self) -> bool {
        match self {
            BonsaiMFVerifyResult::Valid { .. } | BonsaiMFVerifyResult::ValidDifferentId(..) => true,
            _ => false,
        }
    }

    pub fn is_ignored(&self) -> bool {
        match self {
            BonsaiMFVerifyResult::Ignored(..) => true,
            _ => false,
        }
    }
}

#[derive(Clone)]
pub struct BonsaiMFVerifyDifference {
    // Root manifests in treemanifest hybrid mode use a different ID than what's computed.
    // See the documentation in mercurial_types/if/mercurial_thrift.thrift's HgManifestEnvelope
    // for more.
    pub lookup_mf_id: HgNodeHash,
    // The difference/inconsistency is that expected_mf_id is not the same as roundtrip_mf_id.
    pub expected_mf_id: HgNodeHash,
    pub roundtrip_mf_id: HgNodeHash,
    repo: BlobRepo,
}

impl BonsaiMFVerifyDifference {
    /// What entries changed from the original manifest to the roundtripped one.
    pub fn changes(
        &self,
        ctx: CoreContext,
    ) -> impl Stream<Item = ChangedEntry, Error = Error> + Send {
        let lookup_mf_id = HgManifestId::new(self.lookup_mf_id);
        let roundtrip_mf_id = HgManifestId::new(self.roundtrip_mf_id);
        let original_mf = self.repo.get_manifest_by_nodeid(ctx.clone(), lookup_mf_id);
        let roundtrip_mf = self
            .repo
            .get_manifest_by_nodeid(ctx.clone(), roundtrip_mf_id);
        original_mf
            .join(roundtrip_mf)
            .map({
                cloned!(ctx);
                move |(original_mf, roundtrip_mf)| {
                    changed_entry_stream(ctx, &roundtrip_mf, &original_mf, None)
                }
            })
            .flatten_stream()
    }

    /// Whether there are any changes beyond the root manifest ID being different.
    #[inline]
    pub fn has_changes(&self, ctx: CoreContext) -> impl Future<Item = bool, Error = Error> + Send {
        self.changes(ctx).not_empty()
    }

    /// Whether there are any files that changed.
    #[inline]
    pub fn has_file_changes(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = bool, Error = Error> + Send {
        self.changes(ctx)
            .filter(|item| !item.status.is_tree())
            .not_empty()
    }

    // XXX might need to return repo here if callers want to do direct queries
}

impl fmt::Debug for BonsaiMFVerifyDifference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BonsaiMFVerifyDifference")
            .field("lookup_mf_id", &format!("{}", self.lookup_mf_id))
            .field("expected_mf_id", &format!("{}", self.expected_mf_id))
            .field("roundtrip_mf_id", &format!("{}", self.roundtrip_mf_id))
            .finish()
    }
}

pub struct BonsaiMFVerify {
    pub ctx: CoreContext,
    pub logger: Logger,
    pub repo: BlobRepo,
    pub follow_limit: usize,
    pub ignores: HashSet<HgChangesetId>,
    pub broken_merges_before: Option<DateTime>,
    pub debug_bonsai_diff: bool,
}

impl BonsaiMFVerify {
    /// Verify that a list of changesets roundtrips through bonsai. Returns a stream of
    /// inconsistencies and errors encountered, which completes once verification is complete.
    pub fn verify(
        self,
        start_points: impl IntoIterator<Item = HgChangesetId>,
    ) -> impl Stream<Item = (BonsaiMFVerifyResult, ChangesetVisitMeta), Error = Error> + Send {
        let repo = self.repo.in_memory_writes_READ_DOC_COMMENT();

        visit_changesets(
            self.ctx,
            self.logger,
            repo,
            BonsaiMFVerifyVisitor {
                ignores: Arc::new(self.ignores),
                broken_merges_before: self.broken_merges_before,
                debug_bonsai_diff: self.debug_bonsai_diff,
            },
            start_points,
            self.follow_limit,
        )
    }
}

#[derive(Clone, Debug)]
struct BonsaiMFVerifyVisitor {
    ignores: Arc<HashSet<HgChangesetId>>,
    broken_merges_before: Option<DateTime>,
    debug_bonsai_diff: bool,
}

impl ChangesetVisitor for BonsaiMFVerifyVisitor {
    type Item = BonsaiMFVerifyResult;

    fn visit(
        self,
        ctx: CoreContext,
        logger: Logger,
        repo: BlobRepo,
        changeset: HgBlobChangeset,
        _follow_remaining: usize,
    ) -> BoxFuture<Self::Item, Error> {
        let changeset_id = changeset.get_changeset_id();
        if self.ignores.contains(&changeset_id) {
            debug!(logger, "Changeset ignored");
            return future::ok(BonsaiMFVerifyResult::Ignored(changeset_id)).boxify();
        }

        let broken_merge = match &self.broken_merges_before {
            Some(before) => {
                changeset.p1().is_some() && changeset.p2().is_some() && changeset.time() <= before
            }
            None => false,
        };

        if broken_merge {
            debug!(
                logger,
                "Potentially broken merge -- will check for file changes, not just manifest hash"
            );
        }

        debug!(logger, "Starting bonsai diff computation");

        let parents_fut = repo
            .get_changeset_parents(ctx.clone(), changeset_id)
            .and_then({
                cloned!(ctx, repo);
                move |parent_hashes| {
                    let changesets = parent_hashes.into_iter().map(move |parent_id| {
                        repo.get_changeset_by_changesetid(ctx.clone(), parent_id)
                    });
                    future::join_all(changesets)
                }
            });

        // Convert to bonsai first.
        let bonsai_diff_fut = parents_fut.and_then({
            cloned!(ctx, repo);
            move |parents| {
                let mut parents = parents.into_iter();
                let p1: Option<_> = parents.next();
                let p2: Option<_> = parents.next();

                let root_entry = get_root_entry(&repo, &changeset);
                let p1_entry = p1.map(|parent| get_root_entry(&repo, &parent));
                let p2_entry = p2.map(|parent| get_root_entry(&repo, &parent));
                let manifest_p1 = p1_entry
                    .as_ref()
                    .map(|entry| entry.get_hash().into_nodehash());
                let manifest_p2 = p2_entry
                    .as_ref()
                    .map(|entry| entry.get_hash().into_nodehash());

                // Also fetch the manifest as we're interested in the computed node id.
                let root_mf_id = HgManifestId::new(root_entry.get_hash().into_nodehash());
                let root_mf_fut =
                    BlobManifest::load(ctx.clone(), &repo.get_blobstore(), root_mf_id);

                bonsai_diff(ctx.clone(), root_entry, p1_entry, p2_entry)
                    .collect()
                    .join(root_mf_fut)
                    .and_then(move |(diff, root_mf)| match root_mf {
                        Some(root_mf) => Ok((diff, root_mf, manifest_p1, manifest_p2)),
                        None => bail_msg!(
                            "internal error: didn't find root manifest id {}",
                            root_mf_id
                        ),
                    })
            }
        });

        bonsai_diff_fut
            .and_then({
                let logger = logger.clone();
                move |(diff_result, root_mf, manifest_p1, manifest_p2)| {
                    let diff_count = diff_result.len();
                    debug!(
                        logger,
                        "Computed diff ({} entries), now applying it", diff_count,
                    );
                    if self.debug_bonsai_diff {
                        for diff in &diff_result {
                            debug!(logger, "diff result: {}", diff);
                        }
                    }

                    apply_diff(
                        ctx.clone(),
                        logger.clone(),
                        repo.clone(),
                        diff_result,
                        manifest_p1,
                        manifest_p2,
                    )
                    .and_then(move |roundtrip_mf_id| {
                        let lookup_mf_id = root_mf.node_id();
                        let computed_mf_id = root_mf.computed_node_id();
                        debug!(
                            logger,
                            "Saving complete: initial computed manifest ID: {} (original {}), \
                             roundtrip: {}",
                            computed_mf_id,
                            lookup_mf_id,
                            roundtrip_mf_id,
                        );

                        // If there's no diff, memory_manifest will return the same ID as the
                        // parent, which will be the lookup ID, not the computed one.
                        let expected_mf_id = if diff_count == 0 {
                            lookup_mf_id
                        } else {
                            computed_mf_id
                        };
                        if roundtrip_mf_id == expected_mf_id {
                            Either::A(future::ok(BonsaiMFVerifyResult::Valid {
                                lookup_mf_id,
                                computed_mf_id: roundtrip_mf_id,
                            }))
                        } else {
                            let difference = BonsaiMFVerifyDifference {
                                lookup_mf_id,
                                expected_mf_id,
                                roundtrip_mf_id,
                                repo,
                            };

                            if broken_merge {
                                // This is a (potentially) broken merge. Ignore tree changes and
                                // only check for file changes.
                                Either::B(Either::A(difference.has_file_changes(ctx).map(
                                    move |has_file_changes| {
                                        if has_file_changes {
                                            BonsaiMFVerifyResult::Invalid(difference)
                                        } else {
                                            BonsaiMFVerifyResult::ValidDifferentId(difference)
                                        }
                                    },
                                )))
                            } else if diff_count == 0 {
                                // This is an empty changeset. Mercurial is relatively inconsistent
                                // about creating new manifest nodes for such changesets, so it can
                                // happen.
                                Either::B(Either::B(difference.has_changes(ctx).map(
                                    move |has_changes| {
                                        if has_changes {
                                            BonsaiMFVerifyResult::Invalid(difference)
                                        } else {
                                            BonsaiMFVerifyResult::ValidDifferentId(difference)
                                        }
                                    },
                                )))
                            } else {
                                Either::A(future::ok(BonsaiMFVerifyResult::Invalid(difference)))
                            }
                        }
                    })
                }
            })
            .boxify()
    }
}

fn apply_diff(
    ctx: CoreContext,
    _logger: Logger,
    repo: BlobRepo,
    diff_result: Vec<BonsaiDiffResult>,
    manifest_p1: Option<HgNodeHash>,
    manifest_p2: Option<HgNodeHash>,
) -> impl Future<Item = HgNodeHash, Error = Error> + Send {
    let changes: Vec<_> = diff_result
        .into_iter()
        .map(|result| (result.path().clone(), make_entry(&repo, &result)))
        .collect();
    derive_hg_manifest(
        ctx,
        repo,
        IncompleteFilenodes::new(),
        vec![manifest_p1, manifest_p2]
            .into_iter()
            .flatten()
            .map(HgManifestId::new),
        changes,
    )
    .map(|manifest_id| manifest_id.into_nodehash())
}

// XXX should this be in a more central place?
fn make_entry(repo: &BlobRepo, diff_result: &BonsaiDiffResult) -> Option<HgBlobEntry> {
    use self::BonsaiDiffResult::*;

    match diff_result {
        Changed(path, ft, entry_id) | ChangedReusedId(path, ft, entry_id) => {
            let blobstore = repo.get_blobstore();
            let basename = path.basename().clone();
            let hash = entry_id.into_nodehash();
            Some(HgBlobEntry::new(blobstore, basename, hash, Type::File(*ft)))
        }
        Deleted(_path) => None,
    }
}

#[inline]
fn get_root_entry(repo: &BlobRepo, changeset: &HgBlobChangeset) -> Box<dyn HgEntry + Sync> {
    let manifest_id = changeset.manifestid();
    Box::new(repo.get_root_entry(manifest_id))
}
