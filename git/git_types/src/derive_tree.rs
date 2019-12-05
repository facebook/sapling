/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use cloned::cloned;
use context::CoreContext;
use futures::{stream::futures_unordered, Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use manifest::derive_manifest;
use std::collections::HashMap;
use std::convert::TryInto;
use std::sync::Arc;

use blobrepo::BlobRepo;
use blobstore::{Blobstore, Storable};
use derived_data::{BonsaiDerived, BonsaiDerivedMapping};
use filestore::{self, FetchKey};
use mononoke_types::{BonsaiChangeset, ChangesetId, MPath};

use crate::errors::ErrorKind;
use crate::{BlobHandle, Tree, TreeBuilder, TreeHandle};

#[derive(Clone)]
pub struct TreeMapping {
    blobstore: Arc<dyn Blobstore>,
}

impl TreeMapping {
    pub fn new(blobstore: Arc<dyn Blobstore>) -> Self {
        Self { blobstore }
    }

    fn root_key(&self, cs_id: ChangesetId) -> String {
        format!("git.derived_root.{}", cs_id)
    }

    fn fetch_root(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> impl Future<Item = Option<(ChangesetId, TreeHandle)>, Error = Error> {
        self.blobstore
            .get(ctx, self.root_key(cs_id))
            .and_then(move |bytes| match bytes {
                Some(bytes) => bytes.try_into().map(|handle| Some((cs_id, handle))),
                None => Ok(None),
            })
    }
}

impl BonsaiDerivedMapping for TreeMapping {
    type Value = TreeHandle;

    fn get(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<HashMap<ChangesetId, Self::Value>, Error> {
        let gets = csids
            .into_iter()
            .map(|cs_id| self.fetch_root(ctx.clone(), cs_id));

        futures_unordered(gets)
            .filter_map(|maybe_handle| maybe_handle)
            .collect_to()
            .boxify()
    }

    fn put(&self, ctx: CoreContext, csid: ChangesetId, root: Self::Value) -> BoxFuture<(), Error> {
        self.blobstore.put(ctx, self.root_key(csid), root.into())
    }
}

impl BonsaiDerived for TreeHandle {
    const NAME: &'static str = "git_trees";

    fn derive_from_parents(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> BoxFuture<Self, Error> {
        let blobstore = repo.get_blobstore();
        let changes = get_file_changes(&blobstore, &ctx, bonsai);
        changes
            .and_then(move |changes| derive_git_manifest(ctx, blobstore, parents, changes))
            .boxify()
    }
}

fn derive_git_manifest<B: Blobstore + Clone>(
    ctx: CoreContext,
    blobstore: B,
    parents: Vec<TreeHandle>,
    changes: Vec<(MPath, Option<BlobHandle>)>,
) -> impl Future<Item = TreeHandle, Error = Error> {
    derive_manifest(
        ctx.clone(),
        blobstore.clone(),
        parents,
        changes,
        {
            cloned!(ctx, blobstore);
            move |tree_info| {
                let members = tree_info
                    .subentries
                    .into_iter()
                    .map(|(p, (_, entry))| (p, entry.into()))
                    .collect();

                let tree: Tree = TreeBuilder::new(members).into();

                tree.store(ctx.clone(), &blobstore)
                    .map(|handle| ((), handle))
            }
        },
        {
            // NOTE: A None leaf will happen in derive_manifest if the parents have conflicting
            // leaves. However, since we're deriving from a Bonsai changeset and using our Git Tree
            // manifest which has leaves that are equivalent derived to their Bonsai
            // representation, that won't happen.
            |leaf_info| {
                leaf_info
                    .leaf
                    .ok_or(ErrorKind::TreeDerivationFailed.into())
                    .map(|l| ((), l))
            }
        },
    )
    .and_then(move |handle| match handle {
        Some(handle) => Ok(handle).into_future().left_future(),
        None => {
            let tree: Tree = TreeBuilder::default().into();
            tree.store(ctx.clone(), &blobstore).right_future()
        }
    })
}

pub fn get_file_changes<B: Blobstore + Clone>(
    blobstore: &B,
    ctx: &CoreContext,
    bcs: BonsaiChangeset,
) -> impl Future<Item = Vec<(MPath, Option<BlobHandle>)>, Error = Error> {
    let futs = bcs
        .into_mut()
        .file_changes
        .into_iter()
        .map(|(mpath, maybe_file_change)| match maybe_file_change {
            Some(file_change) => {
                let t = file_change.file_type();
                let k = FetchKey::Canonical(file_change.content_id());
                filestore::get_metadata(blobstore, ctx.clone(), &k)
                    .and_then(|r| r.ok_or(ErrorKind::ContentMissing(k).into()))
                    .map(move |m| (mpath, Some(BlobHandle::new(m, t))))
                    .left_future()
            }
            None => Ok((mpath, None)).into_future().right_future(),
        });

    futures_unordered(futs).collect()
}

#[cfg(test)]
mod test {
    use super::*;
    use anyhow::format_err;
    use fbinit::FacebookInit;
    use filestore::Alias;
    use futures_util::compat::{Future01CompatExt, Stream01CompatExt};
    use futures_util::try_stream::TryStreamExt;
    use git2::{Oid, Repository};
    use manifest::ManifestOps;
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;
    use tempdir::TempDir;
    use tokio_preview as tokio;

    /// This function creates a new Git tree from the fixture's master Bonsai bookmark,
    /// materializes it to disk, then verifies that libgit produces the same Git tree for it.
    async fn run_tree_derivation_for_fixture<F>(fb: FacebookInit, fixture: F) -> Result<(), Error>
    where
        F: FnOnce(FacebookInit) -> BlobRepo,
    {
        let ctx = CoreContext::test_mock(fb);
        let repo = fixture(fb);
        let tree_mapping = TreeMapping::new(repo.get_blobstore().boxed());

        let bcs_id = repo
            .get_bonsai_bookmark(ctx.clone(), &("master".try_into()?))
            .compat()
            .await?
            .ok_or(format_err!("no master"))?;

        let tree = TreeHandle::derive(ctx.clone(), repo.clone(), tree_mapping, bcs_id)
            .compat()
            .await?;

        let leaves = tree
            .list_leaf_entries(ctx.clone(), repo.get_blobstore())
            .compat()
            .try_collect::<Vec<_>>()
            .await?;

        let tmp_dir = TempDir::new("git_types_test")?;
        let root_path = tmp_dir.path();
        let git = Repository::init(&root_path)?;
        let mut index = git.index()?;

        for (mpath, blob_handle) in leaves.into_iter() {
            let mpath = match mpath {
                Some(mpath) => mpath,
                None => {
                    continue;
                }
            };

            let blob = filestore::fetch(
                &repo.get_blobstore(),
                ctx.clone(),
                &FetchKey::Aliased(Alias::GitSha1(*blob_handle.oid())),
            )
            .compat()
            .await?
            .ok_or(format_err!("Missing blob"))?
            .compat()
            .try_concat()
            .await?;

            let path = &mpath.to_string();
            let path = Path::new(&path);
            File::create(&root_path.join(&path))?.write_all(&blob)?;

            index.add_path(&path)?;
        }

        let git_oid = index.write_tree()?;
        let derived_tree_oid = Oid::from_bytes(tree.oid().as_ref())?;
        assert_eq!(git_oid, derived_tree_oid);

        tmp_dir.close()?;

        Ok(())
    }

    macro_rules! impl_test {
        ($fixture:ident) => {
            #[fbinit::test]
            async fn $fixture(fb: FacebookInit) -> Result<(), Error> {
                run_tree_derivation_for_fixture(fb, fixtures::$fixture::getrepo).await
            }
        };
    }

    impl_test!(linear);
    impl_test!(branch_even);
    impl_test!(branch_uneven);
    impl_test!(branch_wide);
    impl_test!(merge_even);
    impl_test!(many_files_dirs);
    impl_test!(merge_uneven);
    impl_test!(unshared_merge_even);
    impl_test!(unshared_merge_uneven);

    #[fbinit::test]
    async fn many_diamonds(fb: FacebookInit) -> Result<(), Error> {
        run_tree_derivation_for_fixture(fb, |fb| {
            let mut runtime = ::tokio::runtime::Runtime::new().unwrap();
            fixtures::many_diamonds::getrepo(fb, &mut runtime)
        })
        .await
    }
}
