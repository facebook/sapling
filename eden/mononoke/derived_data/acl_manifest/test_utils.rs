/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use derivation_queue_thrift::DerivationPriority;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use mononoke_types::ChangesetId;
use mononoke_types::MPathElement;
use mononoke_types::acl_manifest::AclManifestDirectoryEntry;
use mononoke_types::acl_manifest::AclManifestDirectoryRestriction;
use mononoke_types::acl_manifest::AclManifestEntry;
use mononoke_types::typed_hash::AclManifestEntryBlobId;
use mononoke_types::typed_hash::AclManifestId;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use tests_utils::CreateCommitContext;

use crate::RootAclManifestId;

#[facet::container]
pub(crate) struct TestRepo(
    dyn BonsaiHgMapping,
    dyn Bookmarks,
    CommitGraph,
    dyn CommitGraphWriter,
    RepoDerivedData,
    RepoBlobstore,
    FilestoreConfig,
    RepoIdentity,
);

pub(crate) const SLACL_PROJECT1: &[u8] =
    b"repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project1\"\n";

pub(crate) const SLACL_PROJECT2: &[u8] =
    b"repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project2\"\n";

pub(crate) const SLACL_PROJECT1_WITH_GROUP: &[u8] =
    b"repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project1\"\npermission_request_group = \"GROUP:my_amp_group\"\n";

/// Generate unique SLACL content for index `i` to avoid content-address
/// deduplication in tests that measure per-restriction-root blob costs.
pub(crate) fn unique_slacl(i: usize) -> &'static [u8] {
    Box::leak(
        format!("repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project_{i}\"\n")
            .into_bytes()
            .into_boxed_slice(),
    )
}

// ---------------------------------------------------------------------------
// Test framework types
// ---------------------------------------------------------------------------

/// Describes a file change in a commit.
pub(crate) enum Change<'a> {
    /// Add or modify a file with the given content.
    Add(&'a str, &'a [u8]),
    /// Delete a file.
    Delete(&'a str),
}

/// Declarative expected manifest tree node.
/// Used with `pretty_assertions::assert_eq` for clear diffs on failure.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ExpectedNode {
    pub name: String,
    pub is_restricted: bool,
    pub has_restricted_descendants: bool,
    pub acl: Option<String>,
    pub permission_request_group: Option<String>,
    pub children: Vec<ExpectedNode>,
}

/// Build an expected manifest node.
/// `acl` being `Some` marks it as restricted. `has_restricted_descendants`
/// is auto-computed from children.
pub(crate) fn node(name: &str, acl: Option<&str>, children: Vec<ExpectedNode>) -> ExpectedNode {
    let has_restricted_descendants = children
        .iter()
        .any(|c| c.is_restricted || c.has_restricted_descendants);
    ExpectedNode {
        name: name.to_string(),
        is_restricted: acl.is_some(),
        has_restricted_descendants,
        acl: acl.map(|s| s.to_string()),
        permission_request_group: None,
        children,
    }
}

/// Result of setting up a repo and deriving manifests.
pub(crate) struct DerivedManifests {
    pub ctx: CoreContext,
    pub repo: TestRepo,
    /// Manifest IDs for each commit, in order.
    pub manifest_ids: Vec<RootAclManifestId>,
}

impl DerivedManifests {
    /// Get the actual tree for the i-th commit's manifest.
    pub async fn tree(&self, index: usize) -> Result<Vec<ExpectedNode>> {
        actual_tree(&self.ctx, &self.repo, self.manifest_ids[index].inner_id()).await
    }

    /// Get the last manifest's tree.
    pub async fn last_tree(&self) -> Result<Vec<ExpectedNode>> {
        self.tree(self.manifest_ids.len() - 1).await
    }

    /// Get the last manifest ID.
    pub fn last_id(&self) -> &RootAclManifestId {
        self.manifest_ids
            .last()
            .expect("at least one commit required")
    }
}

// ---------------------------------------------------------------------------
// Test framework functions
// ---------------------------------------------------------------------------

/// Create a repo, create commits in sequence, derive AclManifest for each
/// using incremental derivation (uses parent manifests).
pub(crate) async fn setup_and_derive(
    fb: FacebookInit,
    commits: Vec<Vec<Change<'_>>>,
) -> Result<DerivedManifests> {
    setup_and_derive_inner(fb, commits, false).await
}

/// Create a repo, create commits in sequence, derive AclManifest for each
/// from scratch (untopologically — ignores parent manifests).
pub(crate) async fn setup_and_derive_from_scratch(
    fb: FacebookInit,
    commits: Vec<Vec<Change<'_>>>,
) -> Result<DerivedManifests> {
    setup_and_derive_inner(fb, commits, true).await
}

async fn setup_and_derive_inner(
    fb: FacebookInit,
    commits: Vec<Vec<Change<'_>>>,
    from_scratch: bool,
) -> Result<DerivedManifests> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let manifest_ids = {
        let ctx_ref = &ctx;
        let repo_ref = &repo;
        stream::iter(commits)
            .map(Ok::<_, anyhow::Error>)
            .try_fold(
                (Vec::new(), None::<ChangesetId>),
                |(mut ids, parent_cs), changes| async move {
                    let commit = changes.into_iter().fold(
                        match parent_cs {
                            Some(p) => CreateCommitContext::new(ctx_ref, repo_ref, vec![p]),
                            None => CreateCommitContext::new_root(ctx_ref, repo_ref),
                        },
                        |commit, change| match change {
                            Change::Add(path, content) => commit.add_file(path, content),
                            Change::Delete(path) => commit.delete_file(path),
                        },
                    );
                    let cs_id = commit.commit().await?;
                    let root_id = if from_scratch {
                        derive_untopologically(ctx_ref, repo_ref, cs_id).await?
                    } else {
                        derive(ctx_ref, repo_ref, cs_id).await?
                    };
                    ids.push(root_id);
                    Ok((ids, Some(cs_id)))
                },
            )
            .await?
            .0
    };

    Ok(DerivedManifests {
        ctx,
        repo,
        manifest_ids,
    })
}

/// Derive AclManifest for a changeset (incremental — uses parent manifests).
pub(crate) async fn derive(
    ctx: &CoreContext,
    repo: &TestRepo,
    cs_id: ChangesetId,
) -> Result<RootAclManifestId> {
    Ok(repo
        .repo_derived_data()
        .derive::<RootAclManifestId>(ctx, cs_id, DerivationPriority::LOW)
        .await?)
}

/// Derive AclManifest from scratch (untopologically — ignores parent manifests).
pub(crate) async fn derive_untopologically(
    ctx: &CoreContext,
    repo: &TestRepo,
    cs_id: ChangesetId,
) -> Result<RootAclManifestId> {
    Ok(repo
        .repo_derived_data()
        .manager()
        .unsafe_derive_untopologically::<RootAclManifestId>(ctx, cs_id, None)
        .await?)
}

/// Pre-derive AclManifest's dependencies (BSSM V3 + Fsnodes) so that
/// subsequent blobstore counter snapshots only measure AclManifest derivation.
pub(crate) async fn derive_deps(
    ctx: &CoreContext,
    repo: &TestRepo,
    cs_id: ChangesetId,
) -> Result<()> {
    use basename_suffix_skeleton_manifest_v3::RootBssmV3DirectoryId;
    use fsnodes::RootFsnodeId;

    repo.repo_derived_data()
        .derive::<RootBssmV3DirectoryId>(ctx, cs_id, DerivationPriority::LOW)
        .await?;
    repo.repo_derived_data()
        .derive::<RootFsnodeId>(ctx, cs_id, DerivationPriority::LOW)
        .await?;
    Ok(())
}

/// Recursively walk the manifest tree and build a sorted `Vec<ExpectedNode>`.
pub(crate) async fn actual_tree(
    ctx: &CoreContext,
    repo: &TestRepo,
    id: &AclManifestId,
) -> Result<Vec<ExpectedNode>> {
    let manifest = id.load(ctx, repo.repo_blobstore()).await?;

    // Filter to only Directory entries (skip AclFile leaf entries).
    let dir_entries: Vec<(MPathElement, AclManifestDirectoryEntry)> = manifest
        .into_subentries(ctx, repo.repo_blobstore())
        .try_filter_map(|(name, entry)| async move {
            match entry {
                AclManifestEntry::AclFile(_) => Ok(None),
                AclManifestEntry::Directory(dir) => Ok(Some((name, dir))),
            }
        })
        .try_collect()
        .await?;

    let mut nodes: Vec<ExpectedNode> = stream::iter(dir_entries)
        .then(|(name, dir)| async move {
            let (acl, permission_request_group) = if dir.is_restricted {
                let child_manifest = dir.id.load(ctx, repo.repo_blobstore()).await?;
                match child_manifest.restriction {
                    AclManifestDirectoryRestriction::Restricted(r) => {
                        let blob = r.entry_blob_id.load(ctx, repo.repo_blobstore()).await?;
                        (Some(blob.repo_region_acl), blob.permission_request_group)
                    }
                    AclManifestDirectoryRestriction::Unrestricted => (None, None),
                }
            } else {
                (None, None)
            };

            let children = Box::pin(actual_tree(ctx, repo, &dir.id)).await?;

            Ok::<_, anyhow::Error>(ExpectedNode {
                name: String::from_utf8(name.as_ref().to_vec())?,
                is_restricted: dir.is_restricted,
                has_restricted_descendants: dir.has_restricted_descendants,
                acl,
                permission_request_group,
                children,
            })
        })
        .try_collect()
        .await?;

    nodes.sort();
    Ok(nodes)
}

/// Extract entry_blob_id from a restricted manifest, or error if unrestricted.
pub(crate) async fn expect_restricted(
    ctx: &CoreContext,
    repo: &TestRepo,
    id: &AclManifestId,
) -> Result<AclManifestEntryBlobId> {
    let manifest = id.load(ctx, repo.repo_blobstore()).await?;
    match manifest.restriction {
        AclManifestDirectoryRestriction::Restricted(r) => Ok(r.entry_blob_id),
        AclManifestDirectoryRestriction::Unrestricted => Err(anyhow::anyhow!(
            "expected manifest {id} to be restricted, got unrestricted"
        )),
    }
}

/// Load directory entries for a manifest ID (filtering out AclFile leaf entries).
pub(crate) async fn load_entries(
    ctx: &CoreContext,
    repo: &TestRepo,
    id: &AclManifestId,
) -> Result<Vec<(MPathElement, AclManifestDirectoryEntry)>> {
    let manifest = id.load(ctx, repo.repo_blobstore()).await?;
    manifest
        .into_subentries(ctx, repo.repo_blobstore())
        .try_filter_map(|(name, entry)| async move {
            match entry {
                AclManifestEntry::AclFile(_) => Ok(None),
                AclManifestEntry::Directory(dir) => Ok(Some((name, dir))),
            }
        })
        .try_collect()
        .await
}

// ---------------------------------------------------------------------------
// Counting blobstore — tracks gets/puts independently of CoreContext
// ---------------------------------------------------------------------------

use std::sync::Arc;

use blobstore::Blobstore;
pub(crate) use counting_blob::BlobstoreCounters;
use counting_blob::CountingBlobstore;

/// A test repo paired with shared blob counters.
pub(crate) struct CountedTestRepo {
    pub repo: TestRepo,
    pub counters: Arc<BlobstoreCounters>,
}

/// Build a test repo whose blobstore counts gets/puts via shared atomics.
/// These counters are independent of CoreContext perf counters and survive
/// context resets inside the derivation manager.
pub(crate) async fn build_counted_test_repo(fb: FacebookInit) -> Result<CountedTestRepo> {
    use memblob::Memblob;

    let counters = Arc::new(BlobstoreCounters::new());
    let blobstore: Arc<dyn Blobstore> =
        Arc::new(CountingBlobstore::new(Memblob::default(), counters.clone()));

    let repo = test_repo_factory::TestRepoFactory::new(fb)?
        .with_blobstore(blobstore)
        .build()
        .await?;

    Ok(CountedTestRepo { repo, counters })
}
