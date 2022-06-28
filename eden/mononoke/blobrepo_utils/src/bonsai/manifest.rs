/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::changeset::visit_changesets;
use crate::changeset::ChangesetVisitMeta;
use crate::changeset::ChangesetVisitor;
use anyhow::bail;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_override::DangerousOverride;
use blobstore::Blobstore;
use blobstore::Loadable;
use cacheblob::MemWritesBlobstore;
use cloned::cloned;
use context::CoreContext;
use futures::future::try_join;
use futures::FutureExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures_ext::try_boxfuture;
use futures_ext::BoxFuture;
use futures_ext::FutureExt as _;
use futures_ext::StreamExt as _;
use futures_old::future;
use futures_old::future::Either;
use futures_old::Future;
use futures_old::Stream;
use manifest::bonsai_diff;
use manifest::BonsaiDiffFileChange;
use manifest::Diff;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_derived_data::derive_hg_manifest;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::blobs::HgBlobManifest;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mononoke_types::DateTime;
use mononoke_types::FileType;
use slog::debug;
use slog::Logger;
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

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
    ) -> impl Stream<Item = Diff<Entry<HgManifestId, (FileType, HgFileNodeId)>>, Error = Error> + Send
    {
        let lookup_mf_id = HgManifestId::new(self.lookup_mf_id);
        let roundtrip_mf_id = HgManifestId::new(self.roundtrip_mf_id);
        lookup_mf_id
            .diff(ctx, self.repo.get_blobstore(), roundtrip_mf_id)
            .compat()
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
            .filter(|diff| {
                let entry = match diff {
                    Diff::Added(_, entry) | Diff::Removed(_, entry) => entry,
                    Diff::Changed(_, _, entry) => entry,
                };
                match entry {
                    Entry::Leaf(_) => true,
                    Entry::Tree(_) => false,
                }
            })
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
        let repo = self
            .repo
            .dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
                Arc::new(MemWritesBlobstore::new(blobstore))
            });

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

        let mut parents = vec![];
        parents.extend(changeset.p1());
        parents.extend(changeset.p2());
        parents.extend(try_boxfuture!(changeset.step_parents()));

        let parents = parents
            .into_iter()
            .map(|p| {
                let id = HgChangesetId::new(p);
                cloned!(ctx, repo);
                async move { id.load(&ctx, repo.blobstore()).await }
                    .boxed()
                    .compat()
                    .from_err()
                    .map(|cs| cs.manifestid())
            })
            .collect::<Vec<_>>();

        // TODO: Update this to support stepparents
        // Convert to bonsai first.
        let bonsai_diff_fut = future::join_all(parents).and_then({
            cloned!(ctx, repo);
            move |parents| {
                let root_mf_fut = {
                    cloned!(ctx, repo);
                    let mf_id = changeset.manifestid();
                    async move { HgBlobManifest::load(&ctx, repo.blobstore(), mf_id).await }
                };

                try_join(
                    bonsai_diff(
                        ctx.clone(),
                        repo.get_blobstore(),
                        changeset.manifestid(),
                        parents.iter().cloned().collect(),
                    )
                    .try_collect::<Vec<_>>(),
                    root_mf_fut,
                )
                .and_then(move |(diff, root_mf)| async move {
                    match root_mf {
                        Some(root_mf) => Ok((diff, root_mf, parents)),
                        None => bail!(
                            "internal error: didn't find root manifest id {}",
                            changeset.manifestid()
                        ),
                    }
                })
                .boxed()
                .compat()
            }
        });

        bonsai_diff_fut
            .and_then({
                let logger = logger.clone();
                move |(diff_result, root_mf, manifestids)| {
                    let diff_count = diff_result.len();
                    debug!(
                        logger,
                        "Computed diff ({} entries), now applying it", diff_count,
                    );
                    if self.debug_bonsai_diff {
                        for diff in &diff_result {
                            debug!(logger, "diff result: {:?}", diff);
                        }
                    }

                    apply_diff(
                        ctx.clone(),
                        logger.clone(),
                        repo.clone(),
                        diff_result,
                        manifestids,
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
    diff_result: Vec<BonsaiDiffFileChange<HgFileNodeId>>,
    manifestids: Vec<HgManifestId>,
) -> impl Future<Item = HgNodeHash, Error = Error> + Send {
    let changes: Vec<_> = diff_result
        .into_iter()
        .map(|result| (result.path().clone(), make_entry(&result)))
        .collect();
    derive_hg_manifest(ctx, repo.get_blobstore().boxed(), manifestids, changes)
        .boxed()
        .compat()
        .map(|manifest_id| manifest_id.into_nodehash())
}

// XXX should this be in a more central place?
fn make_entry(
    diff_result: &BonsaiDiffFileChange<HgFileNodeId>,
) -> Option<(FileType, HgFileNodeId)> {
    use self::BonsaiDiffFileChange::*;

    match diff_result {
        Changed(_, ft, entry_id) | ChangedReusedId(_, ft, entry_id) => Some((*ft, *entry_id)),
        Deleted(_path) => None,
    }
}
