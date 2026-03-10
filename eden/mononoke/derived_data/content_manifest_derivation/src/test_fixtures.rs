/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use derivation_queue_thrift::DerivationPriority;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use fixtures::TestRepoFixture;
use fsnodes::RootFsnodeId;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;

use crate::RootContentManifestId;
use crate::derive::empty_directory;
use crate::derive_from_predecessor::inner_derive_from_predecessor;

#[facet::container]
struct TestRepo(
    dyn BonsaiHgMapping,
    dyn Bookmarks,
    CommitGraph,
    dyn CommitGraphWriter,
    RepoDerivedData,
    RepoBlobstore,
    FilestoreConfig,
    RepoIdentity,
);

async fn test_for_fixture<F: TestRepoFixture + Send>(fb: FacebookInit) -> Result<()> {
    let ctx = &CoreContext::test_mock(fb);
    let repo: TestRepo = F::get_repo(fb).await;
    let derived_data = repo.repo_derived_data();
    let blobstore = repo.repo_blobstore();
    let all_commits = match repo
        .commit_graph()
        .find_by_prefix(ctx, ChangesetIdPrefix::from_bytes("").unwrap(), 1000)
        .await?
    {
        ChangesetIdsResolvedFromPrefix::Multiple(all_commits) => all_commits,
        other => anyhow::bail!("Unexpected number of commits: {:?}", other),
    };
    let restricted_paths = derived_data
        .manager()
        .derivation_context(None)
        .restricted_paths();
    repo.commit_graph()
        .process_topologically(ctx, all_commits, |cs_id| {
            let restricted_paths = restricted_paths.clone();
            async move {
                let content_manifest_id = derived_data
                    .derive::<RootContentManifestId>(ctx, cs_id, DerivationPriority::LOW)
                    .await?
                    .into_content_manifest_id();

                let fsnode_id = derived_data
                    .derive::<RootFsnodeId>(ctx, cs_id, DerivationPriority::LOW)
                    .await?
                    .into_fsnode_id();

                let from_predecessor = inner_derive_from_predecessor(
                    ctx,
                    &blobstore.boxed(),
                    &restricted_paths,
                    fsnode_id,
                    3,
                )
                .await?;

                let from_predecessor = match from_predecessor {
                    Some(id) => id,
                    None => empty_directory(ctx, blobstore).await?,
                };

                assert_eq!(
                    content_manifest_id, from_predecessor,
                    "ContentManifestId mismatch for changeset {:?}",
                    cs_id,
                );

                Ok(())
            }
        })
        .await?;
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_content_manifest_derive_from_predecessor_fixtures(fb: FacebookInit) {
    futures::try_join!(
        test_for_fixture::<fixtures::Linear>(fb),
        test_for_fixture::<fixtures::BranchEven>(fb),
        test_for_fixture::<fixtures::BranchUneven>(fb),
        test_for_fixture::<fixtures::BranchWide>(fb),
        test_for_fixture::<fixtures::MergeEven>(fb),
        test_for_fixture::<fixtures::ManyFilesDirs>(fb),
        test_for_fixture::<fixtures::MergeUneven>(fb),
        test_for_fixture::<fixtures::UnsharedMergeEven>(fb),
        test_for_fixture::<fixtures::UnsharedMergeUneven>(fb),
        test_for_fixture::<fixtures::ManyDiamonds>(fb),
    )
    .unwrap();
}
