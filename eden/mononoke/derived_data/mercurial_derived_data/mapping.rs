/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bonsai_hg_mapping::BonsaiHgMappingEntry;
use context::CoreContext;
use derived_data::batch::split_bonsais_in_linear_stacks;
use derived_data::batch::FileConflicts;
use derived_data::batch::SplitOptions;
use derived_data::batch::DEFAULT_STACK_FILE_CHANGES_LIMIT;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivationContext;
use futures::future::try_join_all;
use mercurial_types::HgChangesetId;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use slog::debug;
use stats::prelude::*;
use std::collections::HashMap;

define_stats! {
    prefix = "mononoke.derived_data.hgchangesets";
    new_parallel: timeseries(Rate, Sum),
}

use derived_data_service_if::types as thrift;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct MappedHgChangesetId(HgChangesetId);

impl MappedHgChangesetId {
    pub(crate) fn new(hg_changeset_id: HgChangesetId) -> Self {
        MappedHgChangesetId(hg_changeset_id)
    }

    pub fn hg_changeset_id(&self) -> HgChangesetId {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct HgChangesetDeriveOptions {
    pub set_committer_field: bool,
}

#[async_trait]
impl BonsaiDerivable for MappedHgChangesetId {
    const NAME: &'static str = "hgchangesets";

    type Dependencies = dependencies![];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self, Error> {
        if bonsai.is_snapshot() {
            bail!("Can't derive Hg changeset for snapshot")
        }
        let derivation_opts = get_hg_changeset_derivation_options(derivation_ctx);
        crate::derive_hg_changeset::derive_from_parents(
            ctx,
            derivation_ctx.blobstore(),
            bonsai,
            parents,
            &derivation_opts,
        )
        .await
    }

    async fn derive_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
        _gap_size: Option<usize>,
    ) -> Result<HashMap<ChangesetId, Self>> {
        if bonsais.is_empty() {
            return Ok(HashMap::new());
        }

        STATS::new_parallel.add_value(1);
        let linear_stacks = split_bonsais_in_linear_stacks(
            &bonsais,
            SplitOptions {
                file_conflicts: FileConflicts::ChangeDelete,
                copy_info: true,
                file_changes_limit: DEFAULT_STACK_FILE_CHANGES_LIMIT,
            },
        )?;
        let mut res: HashMap<ChangesetId, Self> = HashMap::new();
        let batch_len = bonsais.len();

        let derivation_opts = get_hg_changeset_derivation_options(derivation_ctx);

        let mut bonsais = bonsais;
        for stack in linear_stacks {
            let derived_parents = try_join_all(
                stack
                    .parents
                    .into_iter()
                    .map(|p| derivation_ctx.fetch_unknown_dependency::<Self>(ctx, Some(&res), p)),
            )
            .await?;
            if let Some(item) = stack.stack_items.first() {
                debug!(
                    ctx.logger(),
                    "derive hgchangeset batch at {} (stack of {} from batch of {})",
                    item.cs_id.to_hex(),
                    stack.stack_items.len(),
                    batch_len,
                );
            }

            // after the line below `bonsais` will contain all the bonsais that we are
            // going to derive now, and `left_bonsais` will contain all the bonsais that
            // we are going to derive in the next step
            let left_bonsais = bonsais.split_off(stack.stack_items.len());
            if derived_parents.len() > 1 || bonsais.len() == 1 {
                // we can't derive stack for a merge commit or for a commit that contains renames,
                // so let's derive it without batching
                for bonsai in bonsais {
                    let parents = derivation_ctx
                        .fetch_unknown_parents(ctx, Some(&res), &bonsai)
                        .await?;
                    let cs_id = bonsai.get_changeset_id();
                    let derived = Self::derive_single(ctx, derivation_ctx, bonsai, parents).await?;
                    res.insert(cs_id, derived);
                }
            } else {
                let first = stack.stack_items.first().map(|item| item.cs_id);
                let last = stack.stack_items.last().map(|item| item.cs_id);
                let derived =
                    crate::derive_hg_changeset::derive_simple_hg_changeset_stack_without_copy_info(
                        ctx,
                        derivation_ctx.blobstore(),
                        bonsais,
                        derived_parents.get(0).cloned(),
                        &derivation_opts,
                    )
                    .await
                    .with_context(|| {
                        format!("failed deriving stack of {:?} to {:?}", first, last,)
                    })?;

                res.extend(derived.into_iter().map(|(csid, hg_cs_id)| (csid, hg_cs_id)));
            }
            bonsais = left_bonsais;
        }

        Ok(res)
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        derivation_ctx
            .bonsai_hg_mapping()?
            .add(
                ctx,
                BonsaiHgMappingEntry {
                    hg_cs_id: self.0,
                    bcs_id: changeset_id,
                },
            )
            .await?;
        Ok(())
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        Ok(Self::fetch_batch(ctx, derivation_ctx, &[changeset_id])
            .await?
            .into_iter()
            .next()
            .map(|(_, hg_id)| hg_id))
    }

    async fn fetch_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_ids: &[ChangesetId],
    ) -> Result<HashMap<ChangesetId, Self>> {
        Ok(derivation_ctx
            .bonsai_hg_mapping()?
            .get(ctx, changeset_ids.to_vec().into())
            .await?
            .into_iter()
            .map(|entry| (entry.bcs_id, MappedHgChangesetId(entry.hg_cs_id)))
            .collect())
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::hg_changeset(
            thrift::DerivedDataHgChangeset::mapped_hgchangeset_id(id),
        ) = data
        {
            HgChangesetId::from_thrift(id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::hg_changeset(
            thrift::DerivedDataHgChangeset::mapped_hgchangeset_id(data.0.into_thrift()),
        ))
    }
}

fn get_hg_changeset_derivation_options(
    derivation_ctx: &DerivationContext,
) -> HgChangesetDeriveOptions {
    HgChangesetDeriveOptions {
        set_committer_field: derivation_ctx.config().hg_set_committer_extra,
    }
}

impl_bonsai_derived_via_manager!(MappedHgChangesetId);

#[cfg(test)]
mod test {
    use super::*;
    use crate::DeriveHgChangeset;
    use blobrepo::BlobRepo;
    use bookmarks::BookmarkName;
    use borrowed::borrowed;
    use cloned::cloned;
    use derived_data_manager::BatchDeriveOptions;
    use fbinit::FacebookInit;
    use fixtures::BranchEven;
    use fixtures::BranchUneven;
    use fixtures::BranchWide;
    use fixtures::Linear;
    use fixtures::ManyDiamonds;
    use fixtures::ManyFilesDirs;
    use fixtures::MergeEven;
    use fixtures::MergeUneven;
    use fixtures::TestRepoFixture;
    use fixtures::UnsharedMergeEven;
    use fixtures::UnsharedMergeUneven;
    use futures::compat::Stream01CompatExt;
    use futures::Future;
    use futures::Stream;
    use futures::TryFutureExt;
    use futures::TryStreamExt;
    use repo_derived_data::RepoDerivedDataRef;
    use revset::AncestorsNodeStream;
    use tests_utils::CreateCommitContext;

    fn all_commits_descendants_to_ancestors(
        ctx: CoreContext,
        repo: BlobRepo,
    ) -> impl Stream<Item = Result<(ChangesetId, HgChangesetId), Error>> {
        let master_book = BookmarkName::new("master").unwrap();
        repo.get_bonsai_bookmark(ctx.clone(), &master_book)
            .map_ok(move |maybe_bcs_id| {
                let bcs_id = maybe_bcs_id.unwrap();
                AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), bcs_id.clone())
                    .compat()
                    .and_then(move |new_bcs_id| {
                        cloned!(ctx, repo);
                        async move {
                            let hg_cs_id = repo.derive_hg_changeset(&ctx, new_bcs_id).await?;
                            Result::<_, Error>::Ok((new_bcs_id, hg_cs_id))
                        }
                    })
            })
            .try_flatten_stream()
    }

    async fn verify_repo<F, Fut>(fb: FacebookInit, repo_func: F) -> Result<(), Error>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = BlobRepo>,
    {
        let ctx = CoreContext::test_mock(fb);
        let repo = repo_func().await;
        println!("Processing {}", repo.name());
        borrowed!(ctx, repo);

        let commits_desc_to_anc = all_commits_descendants_to_ancestors(ctx.clone(), repo.clone())
            .try_collect::<Vec<_>>()
            .await?;

        // Recreate repo from scratch and derive everything again
        let repo = repo_func().await;
        let options = BatchDeriveOptions::Parallel { gap_size: None };
        let csids = commits_desc_to_anc
            .clone()
            .into_iter()
            .rev()
            .map(|(cs_id, _)| cs_id)
            .collect::<Vec<_>>();
        let manager = repo.repo_derived_data().manager();

        manager
            .backfill_batch::<MappedHgChangesetId>(ctx, csids.clone(), options, None)
            .await?;
        let batch_derived = manager
            .fetch_derived_batch::<MappedHgChangesetId>(ctx, csids, None)
            .await?;

        for (cs_id, hg_cs_id) in commits_desc_to_anc.into_iter().rev() {
            println!("{} {} {:?}", cs_id, hg_cs_id, batch_derived.get(&cs_id));
            assert_eq!(batch_derived.get(&cs_id).map(|x| x.0), Some(hg_cs_id));
        }

        Ok(())
    }

    #[fbinit::test]
    async fn test_batch_derive(fb: FacebookInit) -> Result<(), Error> {
        verify_repo(fb, || Linear::getrepo(fb)).await?;
        verify_repo(fb, || BranchEven::getrepo(fb)).await?;
        verify_repo(fb, || BranchUneven::getrepo(fb)).await?;
        verify_repo(fb, || BranchWide::getrepo(fb)).await?;
        verify_repo(fb, || ManyDiamonds::getrepo(fb)).await?;
        verify_repo(fb, || ManyFilesDirs::getrepo(fb)).await?;
        verify_repo(fb, || MergeEven::getrepo(fb)).await?;
        verify_repo(fb, || MergeUneven::getrepo(fb)).await?;
        verify_repo(fb, || UnsharedMergeEven::getrepo(fb)).await?;
        verify_repo(fb, || UnsharedMergeUneven::getrepo(fb)).await?;
        // Create a repo with a few empty commits in a row
        verify_repo(fb, || async {
            let repo: BlobRepo = test_repo_factory::build_empty(fb).unwrap();
            let ctx = CoreContext::test_mock(fb);
            let root_empty = CreateCommitContext::new_root(&ctx, &repo)
                .commit()
                .await
                .unwrap();
            let first_empty = CreateCommitContext::new(&ctx, &repo, vec![root_empty])
                .commit()
                .await
                .unwrap();
            let second_empty = CreateCommitContext::new(&ctx, &repo, vec![first_empty])
                .commit()
                .await
                .unwrap();
            let first_non_empty = CreateCommitContext::new(&ctx, &repo, vec![second_empty])
                .add_file("file", "a")
                .commit()
                .await
                .unwrap();
            let third_empty = CreateCommitContext::new(&ctx, &repo, vec![first_non_empty])
                .delete_file("file")
                .commit()
                .await
                .unwrap();
            let fourth_empty = CreateCommitContext::new(&ctx, &repo, vec![third_empty])
                .commit()
                .await
                .unwrap();
            let fifth_empty = CreateCommitContext::new(&ctx, &repo, vec![fourth_empty])
                .commit()
                .await
                .unwrap();

            tests_utils::bookmark(&ctx, &repo, "master")
                .set_to(fifth_empty)
                .await
                .unwrap();
            repo
        })
        .await?;

        verify_repo(fb, || async {
            let repo: BlobRepo = test_repo_factory::build_empty(fb).unwrap();
            let ctx = CoreContext::test_mock(fb);
            let root = CreateCommitContext::new_root(&ctx, &repo)
                .add_file("dir/subdir/to_replace", "one")
                .add_file("dir/subdir/file", "content")
                .add_file("somefile", "somecontent")
                .commit()
                .await
                .unwrap();
            let modify_unrelated = CreateCommitContext::new(&ctx, &repo, vec![root])
                .add_file("dir/subdir/file", "content2")
                .delete_file("somefile")
                .commit()
                .await
                .unwrap();
            let replace_file_with_dir =
                CreateCommitContext::new(&ctx, &repo, vec![modify_unrelated])
                    .delete_file("dir/subdir/to_replace")
                    .add_file("dir/subdir/to_replace/file", "newcontent")
                    .commit()
                    .await
                    .unwrap();

            tests_utils::bookmark(&ctx, &repo, "master")
                .set_to(replace_file_with_dir)
                .await
                .unwrap();
            repo
        })
        .await?;

        // Weird case - let's delete a file that was already replaced with a directory
        verify_repo(fb, || async {
            let repo: BlobRepo = test_repo_factory::build_empty(fb).unwrap();
            let ctx = CoreContext::test_mock(fb);
            let root = CreateCommitContext::new_root(&ctx, &repo)
                .add_file("dir/subdir/to_replace", "one")
                .commit()
                .await
                .unwrap();
            let replace_file_with_dir = CreateCommitContext::new(&ctx, &repo, vec![root])
                .delete_file("dir/subdir/to_replace")
                .add_file("dir/subdir/to_replace/file", "newcontent")
                .commit()
                .await
                .unwrap();
            let noop_delete = CreateCommitContext::new(&ctx, &repo, vec![replace_file_with_dir])
                .delete_file("dir/subdir/to_replace")
                .commit()
                .await
                .unwrap();
            let second_noop_delete = CreateCommitContext::new(&ctx, &repo, vec![noop_delete])
                .delete_file("dir/subdir/to_replace")
                .commit()
                .await
                .unwrap();

            tests_utils::bookmark(&ctx, &repo, "master")
                .set_to(second_noop_delete)
                .await
                .unwrap();
            repo
        })
        .await?;

        // Add renames
        verify_repo(fb, || async {
            let repo: BlobRepo = test_repo_factory::build_empty(fb).unwrap();
            let ctx = CoreContext::test_mock(fb);
            let root = CreateCommitContext::new_root(&ctx, &repo)
                .add_file("dir", "one")
                .commit()
                .await
                .unwrap();
            let renamed = CreateCommitContext::new(&ctx, &repo, vec![root])
                .add_file_with_copy_info("copied_dir", "one", (root, "dir"))
                .commit()
                .await
                .unwrap();
            let after_rename = CreateCommitContext::new(&ctx, &repo, vec![renamed])
                .add_file("new_file", "file")
                .commit()
                .await
                .unwrap();

            tests_utils::bookmark(&ctx, &repo, "master")
                .set_to(after_rename)
                .await
                .unwrap();
            repo
        })
        .await?;

        Ok(())
    }
}
