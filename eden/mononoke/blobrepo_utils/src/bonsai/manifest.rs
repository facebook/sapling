/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::Loadable;
use cacheblob::MemWritesBlobstore;
use cloned::cloned;
use context::CoreContext;
use futures::pin_mut;
use futures::stream::FuturesOrdered;
use futures::try_join;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use manifest::bonsai_diff;
use manifest::BonsaiDiffFileChange;
use manifest::Diff;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_derivation::derive_hg_manifest;
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

use crate::changeset::visit_changesets;
use crate::changeset::ChangesetVisitMeta;
use crate::Repo;

#[derive(Clone, Debug)]
pub enum BonsaiMFVerifyResult<R> {
    Valid {
        lookup_mf_id: HgNodeHash,
        computed_mf_id: HgNodeHash,
    },
    // ValidDifferentHash means that the root manifest ID didn't match up, but that that was
    // because of an expected difference in hash that isn't substantive.
    ValidDifferentId(BonsaiMFVerifyDifference<R>),
    Invalid(BonsaiMFVerifyDifference<R>),
    Ignored(HgChangesetId),
}

impl<R> BonsaiMFVerifyResult<R> {
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
pub struct BonsaiMFVerifyDifference<R> {
    // Root manifests in treemanifest hybrid mode use a different ID than what's computed.
    // See the documentation in mercurial_types/if/mercurial_thrift.thrift's HgManifestEnvelope
    // for more.
    pub lookup_mf_id: HgNodeHash,
    // The difference/inconsistency is that expected_mf_id is not the same as roundtrip_mf_id.
    pub expected_mf_id: HgNodeHash,
    pub roundtrip_mf_id: HgNodeHash,
    repo: R,
}

impl<R: Repo> BonsaiMFVerifyDifference<R> {
    /// What entries changed from the original manifest to the roundtripped one.
    pub fn changes(
        &self,
        ctx: CoreContext,
    ) -> impl Stream<Item = Result<Diff<Entry<HgManifestId, (FileType, HgFileNodeId)>>>> + Send
    {
        let lookup_mf_id = HgManifestId::new(self.lookup_mf_id);
        let roundtrip_mf_id = HgManifestId::new(self.roundtrip_mf_id);
        lookup_mf_id.diff(ctx, self.repo.repo_blobstore().clone(), roundtrip_mf_id)
    }

    /// Whether there are any changes beyond the root manifest ID being different.
    #[inline]
    pub async fn has_changes(&self, ctx: CoreContext) -> Result<bool> {
        let stream = self.changes(ctx);
        pin_mut!(stream);
        Ok(stream.next().await.is_some())
    }

    /// Whether there are any files that changed.
    #[inline]
    pub async fn has_file_changes(&self, ctx: CoreContext) -> Result<bool> {
        let stream = self.changes(ctx).try_filter(|diff| {
            cloned!(diff);
            async move {
                let entry = match diff {
                    Diff::Added(_, entry) | Diff::Removed(_, entry) => entry,
                    Diff::Changed(_, _, entry) => entry,
                };
                match entry {
                    Entry::Leaf(_) => true,
                    Entry::Tree(_) => false,
                }
            }
        });
        pin_mut!(stream);
        Ok(stream.next().await.is_some())
    }

    // XXX might need to return repo here if callers want to do direct queries
}

impl<R> fmt::Debug for BonsaiMFVerifyDifference<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BonsaiMFVerifyDifference")
            .field("lookup_mf_id", &format!("{}", self.lookup_mf_id))
            .field("expected_mf_id", &format!("{}", self.expected_mf_id))
            .field("roundtrip_mf_id", &format!("{}", self.roundtrip_mf_id))
            .finish()
    }
}

pub struct BonsaiMFVerify<R> {
    pub ctx: CoreContext,
    pub logger: Logger,
    pub repo: R,
    pub follow_limit: usize,
    pub ignores: HashSet<HgChangesetId>,
    pub broken_merges_before: Option<DateTime>,
    pub debug_bonsai_diff: bool,
}

impl<R: Repo> BonsaiMFVerify<R> {
    /// Verify that a list of changesets roundtrips through bonsai. Returns a stream of
    /// inconsistencies and errors encountered, which completes once verification is complete.
    pub fn verify(
        self,
        start_points: impl IntoIterator<Item = HgChangesetId>,
    ) -> impl Stream<Item = Result<(BonsaiMFVerifyResult<R>, ChangesetVisitMeta)>> + Send {
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
pub struct BonsaiMFVerifyVisitor {
    ignores: Arc<HashSet<HgChangesetId>>,
    broken_merges_before: Option<DateTime>,
    debug_bonsai_diff: bool,
}

impl BonsaiMFVerifyVisitor {
    pub async fn visit<R: Repo>(
        self,
        ctx: CoreContext,
        logger: Logger,
        repo: R,
        changeset: HgBlobChangeset,
    ) -> Result<BonsaiMFVerifyResult<R>> {
        let changeset_id = changeset.get_changeset_id();
        if self.ignores.contains(&changeset_id) {
            debug!(logger, "Changeset ignored");
            return Ok(BonsaiMFVerifyResult::Ignored(changeset_id));
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
        parents.extend(changeset.step_parents()?);

        let parents = parents
            .into_iter()
            .map(|p| {
                let id = HgChangesetId::new(p);
                cloned!(ctx, repo);
                async move {
                    let cs = id.load(&ctx, repo.repo_blobstore()).await?;
                    anyhow::Ok(cs.manifestid())
                }
            })
            .collect::<FuturesOrdered<_>>()
            .try_collect::<Vec<_>>()
            .await?;

        // TODO: Update this to support stepparents
        // Convert to bonsai first.
        let root_mf_fut = {
            cloned!(ctx, repo);
            let mf_id = changeset.manifestid();
            async move { HgBlobManifest::load(&ctx, repo.repo_blobstore(), mf_id).await }
        };

        let (diff, root_mf) = try_join!(
            bonsai_diff(
                ctx.clone(),
                repo.repo_blobstore().clone(),
                changeset.manifestid(),
                parents.iter().cloned().collect(),
            )
            .try_collect::<Vec<_>>(),
            root_mf_fut,
        )?;

        let (diff_result, root_mf, manifestids) = match root_mf {
            Some(root_mf) => (diff, root_mf, parents),
            None => bail!(
                "internal error: didn't find root manifest id {}",
                changeset.manifestid()
            ),
        };

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

        let roundtrip_mf_id =
            apply_diff(ctx.clone(), repo.clone(), diff_result, manifestids).await?;

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
            Ok(BonsaiMFVerifyResult::Valid {
                lookup_mf_id,
                computed_mf_id: roundtrip_mf_id,
            })
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
                if difference.has_file_changes(ctx).await? {
                    Ok(BonsaiMFVerifyResult::Invalid(difference))
                } else {
                    Ok(BonsaiMFVerifyResult::ValidDifferentId(difference))
                }
            } else if diff_count == 0 {
                // This is an empty changeset. Mercurial is relatively inconsistent
                // about creating new manifest nodes for such changesets, so it can
                // happen.
                if difference.has_changes(ctx).await? {
                    Ok(BonsaiMFVerifyResult::Invalid(difference))
                } else {
                    Ok(BonsaiMFVerifyResult::ValidDifferentId(difference))
                }
            } else {
                Ok(BonsaiMFVerifyResult::Invalid(difference))
            }
        }
    }
}

async fn apply_diff(
    ctx: CoreContext,
    repo: impl Repo,
    diff_result: Vec<BonsaiDiffFileChange<HgFileNodeId>>,
    manifestids: Vec<HgManifestId>,
) -> Result<HgNodeHash> {
    let changes: Vec<_> = diff_result
        .into_iter()
        .map(|result| (result.path().clone(), make_entry(&result)))
        .collect();
    let manifest_id =
        derive_hg_manifest(ctx, repo.repo_blobstore_arc(), manifestids, changes).await?;
    Ok(manifest_id.into_nodehash())
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
