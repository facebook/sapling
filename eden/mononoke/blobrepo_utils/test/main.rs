/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use fixtures::*;

// An extra level of nesting is required to avoid clashes between crate and module names.
mod test {
    macro_rules! test_verify {
        ($test_name:ident, $repo:ident) => {
            mod $test_name {
                use std::collections::HashSet;
                use std::sync::Arc;

                use anyhow::Result;
                use blobrepo_hg::BlobRepoHg;
                use blobrepo_override::DangerousOverride;
                use blobrepo_utils::BonsaiMFVerify;
                use blobrepo_utils::BonsaiMFVerifyResult;
                use blobstore::Blobstore;
                use bonsai_hg_mapping::BonsaiHgMapping;
                use bookmarks::Bookmarks;
                use commit_graph::CommitGraph;
                use commit_graph::CommitGraphWriter;
                use context::CoreContext;
                use fbinit::FacebookInit;
                use filestore::FilestoreConfig;
                use fixtures::TestRepoFixture;
                use futures::stream::FuturesOrdered;
                use futures::TryFutureExt;
                use futures::TryStreamExt;
                use mononoke_macros::mononoke;
                use repo_blobstore::RepoBlobstore;
                use repo_derived_data::RepoDerivedData;
                use repo_identity::RepoIdentity;

                use crate::$repo;

                #[facet::container]
                #[derive(Clone)]
                struct TestRepo {
                    #[facet]
                    bonsai_hg_mapping: dyn BonsaiHgMapping,

                    #[facet]
                    bookmarks: dyn Bookmarks,

                    #[facet]
                    commit_graph: CommitGraph,

                    #[facet]
                    commit_graph_writer: dyn CommitGraphWriter,

                    #[facet]
                    repo_blobstore: RepoBlobstore,

                    #[facet]
                    repo_identity: RepoIdentity,

                    #[facet]
                    repo_derived_data: RepoDerivedData,

                    #[facet]
                    filestore_config: FilestoreConfig,
                }

                impl DangerousOverride<Arc<dyn Blobstore>> for TestRepo {
                    fn dangerous_override<F>(&self, modify: F) -> Self
                    where
                        F: FnOnce(Arc<dyn Blobstore>) -> Arc<dyn Blobstore>,
                    {
                        let blobstore = RepoBlobstore::new_with_wrapped_inner_blobstore(
                            self.repo_blobstore.as_ref().clone(),
                            modify,
                        );
                        let repo_derived_data = Arc::new(
                            self.repo_derived_data
                                .with_replaced_blobstore(blobstore.clone()),
                        );
                        let repo_blobstore = Arc::new(blobstore);
                        Self {
                            repo_blobstore,
                            repo_derived_data,
                            ..self.clone()
                        }
                    }
                }

                #[mononoke::fbinit_test]
                async fn test(fb: FacebookInit) -> Result<()> {
                    let ctx = CoreContext::test_mock(fb);

                    let repo: TestRepo = $repo::get_repo(fb).await;
                    let heads = repo
                        .get_hg_heads_maybe_stale(ctx.clone())
                        .try_collect::<Vec<_>>()
                        .await?;

                    let verify = BonsaiMFVerify {
                        ctx: ctx.clone(),
                        logger: ctx.logger().clone(),
                        repo: repo.clone(),
                        follow_limit: 1024,
                        ignores: HashSet::new(),
                        broken_merges_before: None,
                        debug_bonsai_diff: false,
                    };

                    let results = verify.verify(heads).try_collect::<Vec<_>>().await?;
                    let diffs = results
                        .into_iter()
                        .filter_map(move |(res, meta)| match res {
                            BonsaiMFVerifyResult::Invalid(difference) => {
                                let cs_id = meta.changeset_id;
                                Some(
                                    difference
                                        .changes(ctx.clone())
                                        .try_collect::<Vec<_>>()
                                        .map_ok(move |changes| (cs_id, changes)),
                                )
                            }
                            _ => None,
                        })
                        .collect::<FuturesOrdered<_>>()
                        .try_collect::<Vec<_>>()
                        .await?;

                    let mut failed = false;
                    let mut desc = Vec::new();
                    for (changeset_id, changes) in diffs {
                        failed = true;
                        desc.push(format!("*** Inconsistent roundtrip for {}", changeset_id,));
                        for changed_entry in changes {
                            desc.push(format!("  - Changed entry: {:?}", changed_entry));
                        }
                        desc.push("".to_string());
                    }
                    let desc = desc.join("\n");
                    if failed {
                        panic!(
                            "Inconsistencies detected, roundtrip test failed\n\n{}",
                            desc
                        );
                    }
                    Ok(())
                }
            }
        };
    }

    test_verify!(branch_even, BranchEven);
    test_verify!(branch_uneven, BranchUneven);
    test_verify!(branch_wide, BranchWide);
    test_verify!(linear, Linear);
    test_verify!(merge_even, MergeEven);
    test_verify!(merge_uneven, MergeUneven);
    test_verify!(unshared_merge_even, UnsharedMergeEven);
    test_verify!(unshared_merge_uneven, UnsharedMergeUneven);
}
