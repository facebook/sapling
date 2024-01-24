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
use blobstore::Blobstore;
use blobstore::Storable;
use cloned::cloned;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_service_if::types as thrift;
use futures::future::ready;
use futures::stream::FuturesUnordered;
use futures::stream::TryStreamExt;
use manifest::derive_manifest;
use manifest::flatten_subentries;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;

use crate::errors::MononokeGitError;
use crate::fetch_non_blob_git_object;
use crate::upload_non_blob_git_object;
use crate::BlobHandle;
use crate::Tree;
use crate::TreeBuilder;
use crate::TreeHandle;

fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "git.derived_root.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<TreeHandle>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

#[async_trait]
impl BonsaiDerivable for TreeHandle {
    const VARIANT: DerivableType = DerivableType::GitTree;

    type Dependencies = dependencies![];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self> {
        if bonsai.is_snapshot() {
            bail!("Can't derive TreeHandle for snapshot")
        }
        let blobstore = derivation_ctx.blobstore().clone();
        let cs_id = bonsai.get_changeset_id();
        let changes = get_file_changes(&blobstore, ctx, bonsai).await?;
        // Check whether the git commit for this bonsai commit is already known.
        // If so, then the raw git tree will also exist, as it would have been uploaded
        // alongside the commit. If not, then the raw tree git may not already exist and
        // we should derive it.
        let derive_raw_tree = derivation_ctx
            .bonsai_git_mapping()?
            .get_git_sha1_from_bonsai(ctx, cs_id)
            .await
            .with_context(|| format!("Error in getting Git Sha1 for Bonsai Changeset {}", cs_id))?
            .is_none();
        derive_git_manifest(ctx, blobstore, parents, changes, derive_raw_tree).await
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        let key = format_key(derivation_ctx, changeset_id);
        derivation_ctx.blobstore().put(ctx, key, self.into()).await
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        let key = format_key(derivation_ctx, changeset_id);
        Ok(derivation_ctx
            .blobstore()
            .get(ctx, &key)
            .await?
            .map(TryInto::try_into)
            .transpose()?)
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::tree_handle(thrift::DerivedDataTreeHandle::tree_handle(id)) =
            data
        {
            Self::try_from(id)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::tree_handle(
            thrift::DerivedDataTreeHandle::tree_handle(data.into()),
        ))
    }
}

impl_bonsai_derived_via_manager!(TreeHandle);

async fn derive_git_manifest<B: Blobstore + Clone + 'static>(
    ctx: &CoreContext,
    blobstore: B,
    parents: Vec<TreeHandle>,
    changes: Vec<(NonRootMPath, Option<BlobHandle>)>,
    derive_raw_tree: bool,
) -> Result<TreeHandle, Error> {
    let handle = derive_manifest(
        ctx.clone(),
        blobstore.clone(),
        parents,
        changes,
        {
            cloned!(ctx, blobstore);
            move |tree_info| {
                cloned!(ctx, blobstore);
                async move {
                    let members = flatten_subentries(&ctx, &(), tree_info.subentries)
                        .await?
                        .map(|(p, (_, entry))| (p, entry.into()))
                        .collect();

                    let builder = TreeBuilder::new(members);
                    let (mut tree_bytes_without_header, tree) = builder.into_tree_with_bytes();
                    let oid = tree.handle().oid();
                    let git_hash = oid.to_object_id()?;
                    if derive_raw_tree {
                        // Store the raw git tree before storing the thrift version
                        // Need to prepend the object header before storing the Git tree
                        let mut raw_tree_bytes = oid.prefix();
                        raw_tree_bytes.append(&mut tree_bytes_without_header);
                        upload_non_blob_git_object(
                            &ctx,
                            &blobstore,
                            git_hash.as_ref(),
                            raw_tree_bytes,
                        )
                        .await?;
                    } else {
                        // We don't need to store the raw git tree because it already exists. Validate that the existing tree
                        // is present in the blobstore with the same hash as we computed. If not, then it means that we computed
                        // a thirft Git tree that is different than the stored raw tree. This could be due to a bug so we need to
                        // fail before storing the thrift tree
                        fetch_non_blob_git_object(&ctx, &blobstore, git_hash.as_ref())
                            .await
                            .with_context(|| {
                                format!(
                                    "Raw Git tree with hash {} should have been present already",
                                    git_hash.to_hex()
                                )
                            })?;
                    }
                    // Upload the thrift Git Tree
                    let handle = tree.store(&ctx, &blobstore).await?;
                    Ok(((), handle))
                }
            }
        },
        {
            // NOTE: A None leaf will happen in derive_manifest if the parents have conflicting
            // leaves. However, since we're deriving from a Bonsai changeset and using our Git Tree
            // manifest which has leaves that are equivalent derived to their Bonsai
            // representation, that won't happen.
            |leaf_info| {
                let leaf = leaf_info
                    .leaf
                    .ok_or_else(|| MononokeGitError::TreeDerivationFailed.into())
                    .map(|l| ((), l));
                ready(leaf)
            }
        },
    )
    .await?;

    match handle {
        Some(handle) => Ok(handle),
        None => {
            let tree: Tree = TreeBuilder::default().into();
            tree.store(ctx, &blobstore).await
        }
    }
}

pub async fn get_file_changes<B: Blobstore + Clone>(
    blobstore: &B,
    ctx: &CoreContext,
    bcs: BonsaiChangeset,
) -> Result<Vec<(NonRootMPath, Option<BlobHandle>)>, Error> {
    bcs.into_mut()
        .file_changes
        .into_iter()
        .map(|(mpath, file_change)| async move {
            match file_change.simplify() {
                Some(basic_file_change) => Ok((
                    mpath,
                    Some(BlobHandle::new(ctx, blobstore, basic_file_change).await?),
                )),
                None => Ok((mpath, None)),
            }
        })
        .collect::<FuturesUnordered<_>>()
        .try_collect()
        .await
}

#[cfg(test)]
mod test {
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;
    use std::str::FromStr;

    use anyhow::format_err;
    use bookmarks::BookmarkKey;
    use bookmarks::BookmarksRef;
    use derived_data::BonsaiDerived;
    use fbinit::FacebookInit;
    use filestore::Alias;
    use filestore::FetchKey;
    use fixtures::TestRepoFixture;
    use futures_util::stream::TryStreamExt;
    use git2::Oid;
    use git2::Repository;
    use manifest::ManifestOps;
    use repo_blobstore::RepoBlobstoreArc;
    use repo_derived_data::RepoDerivedDataRef;
    use tempfile::TempDir;

    use super::*;

    /// This function creates a new Git tree from the fixture's master Bonsai bookmark,
    /// materializes it to disk, then verifies that libgit produces the same Git tree for it.
    async fn run_tree_derivation_for_fixture(
        fb: FacebookInit,
        repo: impl BookmarksRef + RepoBlobstoreArc + RepoDerivedDataRef + Send + Sync,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        let bcs_id = repo
            .bookmarks()
            .get(ctx.clone(), &BookmarkKey::from_str("master")?)
            .await?
            .ok_or_else(|| format_err!("no master"))?;

        let tree = TreeHandle::derive(&ctx, &repo, bcs_id).await?;

        let leaves = tree
            .list_leaf_entries(ctx.clone(), repo.repo_blobstore_arc())
            .try_collect::<Vec<_>>()
            .await?;

        let tmp_dir = TempDir::with_prefix("git_types_test.")?;
        let root_path = tmp_dir.path();
        let git = Repository::init(root_path)?;
        let mut index = git.index()?;

        for (mpath, blob_handle) in leaves.into_iter() {
            let blob = filestore::fetch_concat(
                repo.repo_blobstore(),
                &ctx,
                FetchKey::Aliased(Alias::GitSha1(blob_handle.oid().sha1())),
            )
            .await?;

            let path = &mpath.to_string();
            let path = Path::new(&path);
            File::create(&root_path.join(path))?.write_all(&blob)?;

            index.add_path(path)?;
        }

        let git_oid = index.write_tree()?;
        let derived_tree_oid = Oid::from_bytes(tree.oid().as_ref())?;
        assert_eq!(git_oid, derived_tree_oid);

        tmp_dir.close()?;

        Ok(())
    }

    macro_rules! impl_test {
        ($test_name:ident, $fixture:ident) => {
            #[fbinit::test]
            fn $test_name(fb: FacebookInit) -> Result<(), Error> {
                let runtime = tokio::runtime::Runtime::new()?;
                runtime.block_on(async move {
                    let repo = fixtures::$fixture::getrepo(fb).await;
                    run_tree_derivation_for_fixture(fb, repo).await
                })
            }
        };
    }

    impl_test!(linear, Linear);
    impl_test!(branch_even, BranchEven);
    impl_test!(branch_uneven, BranchUneven);
    impl_test!(branch_wide, BranchWide);
    impl_test!(merge_even, MergeEven);
    impl_test!(many_files_dirs, ManyFilesDirs);
    impl_test!(merge_uneven, MergeUneven);
    impl_test!(unshared_merge_even, UnsharedMergeEven);
    impl_test!(unshared_merge_uneven, UnsharedMergeUneven);
    impl_test!(many_diamonds, ManyDiamonds);
}
