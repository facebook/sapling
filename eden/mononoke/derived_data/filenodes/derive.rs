/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::mapping::{FilenodesOnlyPublic, PreparedRootFilenode};
use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use borrowed::borrowed;
use context::CoreContext;
use filenodes::{FilenodeInfo, FilenodeResult, PreparedFilenode};
use futures::{
    compat::Future01CompatExt, future::try_join_all, pin_mut, stream, FutureExt, StreamExt,
    TryFutureExt, TryStreamExt,
};
use futures_util::try_join;
use itertools::{Either, Itertools};
use manifest::{find_intersection_of_diffs_and_parents, Entry};
use mercurial_types::{
    blobs::File, fetch_manifest_envelope, HgChangesetId, HgFileEnvelope, HgFileNodeId,
    HgManifestEnvelope, HgManifestId,
};
use mononoke_types::{ChangesetId, MPath, RepoPath};

pub async fn derive_filenodes(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<FilenodesOnlyPublic, Error> {
    let (_, public_filenode, non_roots) = prepare_filenodes_for_cs(ctx, repo, cs_id).await?;
    if !non_roots.is_empty() {
        if let FilenodeResult::Disabled = add_filenodes(ctx, repo, non_roots).await? {
            return Ok(FilenodesOnlyPublic::Disabled);
        }
    }
    Ok(public_filenode)
}

pub async fn derive_filenodes_in_batch(
    ctx: &CoreContext,
    repo: &BlobRepo,
    batch: Vec<ChangesetId>,
) -> Result<Vec<(ChangesetId, FilenodesOnlyPublic, Vec<PreparedFilenode>)>, Error> {
    stream::iter(
        batch
            .clone()
            .into_iter()
            .map(|cs_id| async move { prepare_filenodes_for_cs(ctx, repo, cs_id).await }),
    )
    .buffered(100)
    .try_collect()
    .await
}

pub async fn prepare_filenodes_for_cs(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<(ChangesetId, FilenodesOnlyPublic, Vec<PreparedFilenode>), Error> {
    let filenodes = generate_all_filenodes(ctx, repo, cs_id).await?;
    if filenodes.is_empty() {
        // This commit didn't create any new filenodes, and it's root manifest is the
        // same as one of the parents (that can happen if this commit is empty).
        // In that case we don't need to insert a root filenode - it will be inserted
        // when parent is derived.
        Ok((
            cs_id,
            FilenodesOnlyPublic::Present {
                root_filenode: None,
            },
            filenodes,
        ))
    } else {
        let (roots, non_roots): (Vec<_>, Vec<_>) =
            filenodes.into_iter().partition_map(classify_filenode);
        let roots_num = roots.len();
        let mut roots = roots.into_iter();

        match (roots.next(), roots.next()) {
            (Some(root_filenode), None) => Ok((
                cs_id,
                FilenodesOnlyPublic::Present {
                    root_filenode: Some(root_filenode),
                },
                non_roots,
            )),
            _ => Err(format_err!(
                "expected exactly one root, found {} for cs_id {}",
                roots_num,
                cs_id.to_string()
            )),
        }
    }
}

pub(crate) async fn add_filenodes(
    ctx: &CoreContext,
    repo: &BlobRepo,
    to_write: Vec<PreparedFilenode>,
) -> Result<FilenodeResult<()>, Error> {
    if to_write.is_empty() {
        return Ok(FilenodeResult::Present(()));
    }
    let filenodes = repo.get_filenodes();
    let repo_id = repo.get_repoid();
    filenodes
        .add_filenodes(ctx.clone(), to_write, repo_id)
        .compat()
        .await
}

pub async fn generate_all_filenodes(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<Vec<PreparedFilenode>, Error> {
    let parents = repo
        .get_changeset_parents_by_bonsai(ctx.clone(), cs_id)
        .await?;

    let root_mf = fetch_root_manifest_id(&ctx, &cs_id, &repo);
    // Bonsai might have > 2 parents, while mercurial supports at most 2.
    // That's fine for us - we just won't generate filenodes for paths that came from
    // stepparents. That means that linknode for these filenodes will point to a stepparent
    let parents = try_join_all(
        parents
            .iter()
            .map(|p| fetch_root_manifest_id(&ctx, p, &repo)),
    );
    let linknode = repo.get_hg_from_bonsai_changeset(ctx.clone(), cs_id);

    let (root_mf, parents, linknode) = try_join!(root_mf, parents, linknode)?;
    let blobstore = repo.get_blobstore();
    (async_stream::stream! {
        borrowed!(ctx, blobstore);
        let s = find_intersection_of_diffs_and_parents(
            ctx.clone(),
            blobstore.clone(),
            root_mf,
            parents.clone(),
        )
        .try_filter_map(|(path, entry, parent_entries)| {
            async move {
                // file entry has file type and file node id. If file type is different but filenode is
                // the same we don't want to create a new filenode, and this filter removes
                // all entries where at least one parent has the same filenode id.
                if let Entry::Leaf((_, hg_filenode_id)) = entry {
                    for parent_entry in parent_entries {
                        if let Entry::Leaf((_, parent_filenode_id)) = parent_entry {
                            if parent_filenode_id == hg_filenode_id {
                                return Ok(None);
                            }
                        }
                    }
                }
                Ok(Some((path, entry)))
            }
        })
        .map_ok(move |(path, entry)| {
            match entry {
                Entry::Tree(hg_mf_id) => fetch_manifest_envelope(ctx, blobstore, hg_mf_id)
                    .map_ok(move |envelope| create_manifest_filenode(path, envelope, linknode))
                    .left_future(),
                Entry::Leaf((_, hg_filenode_id)) => async move {
                    let envelope = hg_filenode_id.load(ctx, blobstore).await?;
                    create_file_filenode(path, envelope, linknode)
                }
                    .right_future(),
            }
        })
        .try_buffer_unordered(100);

        pin_mut!(s);
        while let Some(value) = s.next().await {
            yield value;
        }
    })
    .try_collect()
    .await
}

pub(crate) fn classify_filenode(
    filenode: PreparedFilenode,
) -> Either<PreparedRootFilenode, PreparedFilenode> {
    if filenode.path == RepoPath::RootPath {
        let FilenodeInfo {
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        } = filenode.info;

        Either::Left(PreparedRootFilenode {
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        })
    } else {
        Either::Right(filenode)
    }
}

fn create_manifest_filenode(
    path: Option<MPath>,
    envelope: HgManifestEnvelope,
    linknode: HgChangesetId,
) -> PreparedFilenode {
    let path = match path {
        Some(path) => RepoPath::DirectoryPath(path),
        None => RepoPath::RootPath,
    };
    let filenode = HgFileNodeId::new(envelope.node_id());
    let (p1, p2) = envelope.parents();
    let p1 = p1.map(HgFileNodeId::new);
    let p2 = p2.map(HgFileNodeId::new);

    PreparedFilenode {
        path,
        info: FilenodeInfo {
            filenode,
            p1,
            p2,
            copyfrom: None,
            linknode,
        },
    }
}

fn create_file_filenode(
    path: Option<MPath>,
    envelope: HgFileEnvelope,
    linknode: HgChangesetId,
) -> Result<PreparedFilenode, Error> {
    let path = match path {
        Some(path) => RepoPath::FilePath(path),
        None => {
            return Err(format_err!("unexpected empty file path"));
        }
    };
    let filenode = envelope.node_id();
    let (p1, p2) = envelope.parents();
    let copyfrom = File::extract_copied_from(envelope.metadata())?
        .map(|(path, node)| (RepoPath::FilePath(path), node));

    Ok(PreparedFilenode {
        path,
        info: FilenodeInfo {
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        },
    })
}

async fn fetch_root_manifest_id(
    ctx: &CoreContext,
    cs_id: &ChangesetId,
    repo: &BlobRepo,
) -> Result<HgManifestId, Error> {
    let hg_cs_id = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), *cs_id)
        .await?;
    let hg_cs = hg_cs_id.load(ctx, repo.blobstore()).await?;
    Ok(hg_cs.manifestid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{anyhow, Context};
    use blobrepo_override::DangerousOverride;
    use cloned::cloned;
    use derived_data::{BonsaiDerivable, BonsaiDerived, BonsaiDerivedMapping};
    use fbinit::FacebookInit;
    use filenodes::{FilenodeRangeResult, Filenodes};
    use fixtures::linear;
    use futures::compat::Stream01CompatExt;
    use futures_ext::{BoxFuture, FutureExt as _};
    use manifest::ManifestOps;
    use maplit::hashmap;
    use mononoke_types::{FileType, RepositoryId};
    use revset::AncestorsNodeStream;
    use slog::info;
    use std::{collections::HashMap, sync::Arc};
    use tests_utils::resolve_cs_id;
    use tests_utils::CreateCommitContext;
    use tunables::{with_tunables, MononokeTunables};

    async fn verify_filenodes(
        ctx: &CoreContext,
        repo: &BlobRepo,
        cs_id: ChangesetId,
        expected_paths: Vec<RepoPath>,
    ) -> Result<(), Error> {
        let filenodes = generate_all_filenodes(&ctx, &repo, cs_id).await?;

        assert_eq!(filenodes.len(), expected_paths.len());
        for path in expected_paths {
            assert!(
                filenodes
                    .iter()
                    .find(|filenode| filenode.path == path)
                    .is_some()
            );
        }

        let linknode = repo
            .get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
            .await?;

        for filenode in filenodes {
            assert_eq!(filenode.info.linknode, linknode);
        }
        Ok(())
    }

    async fn test_generate_filenodes_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = blobrepo_factory::new_memblob_empty(None)?;
        let filename = "path";
        let commit = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(filename, "content")
            .commit()
            .await?;

        // Two filenodes - one for root manifest, another for a file
        verify_filenodes(
            &ctx,
            &repo,
            commit,
            vec![RepoPath::RootPath, RepoPath::file(filename)?],
        )
        .await?;

        Ok(())
    }

    #[fbinit::test]
    fn generate_filenodes_simple(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_generate_filenodes_simple(fb))
    }

    async fn test_generate_filenodes_merge(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = blobrepo_factory::new_memblob_empty(None)?;
        let first_p1 = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("path1", "content")
            .commit()
            .await?;

        let first_p2 = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("path2", "content")
            .commit()
            .await?;

        let merge = CreateCommitContext::new(&ctx, &repo, vec![first_p1, first_p2])
            .commit()
            .await?;

        // Only root filenode was added - other filenodes were reused from parents
        verify_filenodes(&ctx, &repo, merge, vec![RepoPath::RootPath]).await?;

        Ok(())
    }

    #[fbinit::test]
    fn generate_filenodes_merge(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_generate_filenodes_merge(fb))
    }

    async fn test_generate_type_change(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = blobrepo_factory::new_memblob_empty(None)?;
        let parent = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("path", "content")
            .commit()
            .await?;

        let child = CreateCommitContext::new(&ctx, &repo, vec![parent])
            .add_file_with_type("path", "content", FileType::Executable)
            .commit()
            .await?;

        // Only root filenode should be changed - change of file type doesn't change filenode
        verify_filenodes(&ctx, &repo, child, vec![RepoPath::RootPath]).await?;

        Ok(())
    }

    #[fbinit::test]
    fn generate_filenodes_type_change(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_generate_type_change(fb))
    }

    async fn test_many_parents(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = blobrepo_factory::new_memblob_empty(None)?;
        let p1 = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("path1", "content")
            .commit()
            .await?;
        let p2 = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("path2", "content")
            .commit()
            .await?;
        let p3 = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("path3", "content")
            .commit()
            .await?;

        let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2, p3])
            .commit()
            .await?;

        info!(ctx.logger(), "checking filenodes for {}", p3);
        verify_filenodes(
            &ctx,
            &repo,
            p3,
            vec![RepoPath::RootPath, RepoPath::file("path3")?],
        )
        .await?;

        info!(ctx.logger(), "checking filenodes for {}", merge);
        verify_filenodes(&ctx, &repo, merge, vec![RepoPath::RootPath]).await?;

        Ok(())
    }

    #[fbinit::test]
    fn many_parents(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_many_parents(fb))
    }

    async fn test_derive_empty_commits(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = blobrepo_factory::new_memblob_empty(None)?;
        let parent_empty = CreateCommitContext::new_root(&ctx, &repo).commit().await?;

        let child_empty = CreateCommitContext::new(&ctx, &repo, vec![parent_empty])
            .add_file("file", "content")
            .commit()
            .await?;

        FilenodesOnlyPublic::derive(&ctx, &repo, child_empty).await?;

        // Make sure they are in the mapping
        let maps = FilenodesOnlyPublic::default_mapping(&ctx, &repo)?
            .get(ctx.clone(), vec![parent_empty, child_empty])
            .await?;

        assert_eq!(maps.len(), 2);
        Ok(())
    }

    #[fbinit::test]
    fn derive_empty_commits(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_derive_empty_commits(fb))
    }

    async fn test_derive_only_empty_commits(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = blobrepo_factory::new_memblob_empty(None)?;

        let parent_empty = CreateCommitContext::new_root(&ctx, &repo).commit().await?;
        let child_empty = CreateCommitContext::new(&ctx, &repo, vec![parent_empty])
            .commit()
            .await?;

        let mapping = FilenodesOnlyPublic::default_mapping(&ctx, &repo)?;
        FilenodesOnlyPublic::derive(&ctx, &repo, child_empty).await?;

        // Make sure they are in the mapping
        let maps = mapping
            .get(ctx.clone(), vec![child_empty, parent_empty])
            .await?;
        assert_eq!(maps.len(), 2);
        Ok(())
    }

    #[fbinit::test]
    fn derive_only_empty_commits(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_derive_only_empty_commits(fb))
    }

    #[fbinit::test]
    fn derive_disabled_filenodes(fb: FacebookInit) -> Result<(), Error> {
        let tunables = MononokeTunables::default();
        tunables.update_bools(&hashmap! {"filenodes_disabled".to_string() => true});

        with_tunables(tunables, || {
            let mut runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(test_derive_disabled_filenodes(fb))
        })
    }

    async fn test_derive_disabled_filenodes(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = blobrepo_factory::new_memblob_empty(None)?;
        let cs = CreateCommitContext::new_root(&ctx, &repo).commit().await?;
        let derived = FilenodesOnlyPublic::derive(&ctx, &repo, cs).await?;
        assert_eq!(derived, FilenodesOnlyPublic::Disabled);

        let mapping = FilenodesOnlyPublic::default_mapping(&ctx, &repo)?;
        let res = mapping.get(ctx.clone(), vec![cs]).await?;

        assert_eq!(res.get(&cs).unwrap(), &FilenodesOnlyPublic::Disabled);

        Ok(())
    }

    #[fbinit::test]
    async fn verify_batch_and_sequential_derive(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo1 = linear::getrepo(fb).await;
        let repo2 = linear::getrepo(fb).await;
        let master_cs_id = resolve_cs_id(&ctx, &repo1, "master").await?;
        let mut cs_ids =
            AncestorsNodeStream::new(ctx.clone(), &repo1.get_changeset_fetcher(), master_cs_id)
                .compat()
                .try_collect::<Vec<_>>()
                .await?;
        cs_ids.reverse();

        let mapping = FilenodesOnlyPublic::default_mapping(&ctx, &repo1)?;
        let batch =
            FilenodesOnlyPublic::batch_derive(&ctx, &repo1, cs_ids.clone(), &mapping, None).await?;

        let sequential = {
            let mut res = HashMap::new();
            for cs in cs_ids.clone() {
                let root_filenode = FilenodesOnlyPublic::derive(&ctx, &repo2, cs).await?;
                res.insert(cs, root_filenode);
            }
            res
        };

        assert_eq!(batch, sequential);
        for cs in cs_ids {
            compare_filenodes(&ctx, &repo1, &repo2, &cs).await?;
        }
        Ok(())
    }

    #[fbinit::test]
    async fn derive_parents_before_children(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = linear::getrepo(fb).await;
        let commit8 =
            resolve_cs_id(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await?;
        let commit8 = repo
            .get_hg_from_bonsai_changeset(ctx.clone(), commit8)
            .await?;
        let repo = repo.dangerous_override(|inner| -> Arc<dyn Filenodes> {
            Arc::new(FilenodesWrapper {
                inner,
                cs_id: commit8,
            })
        });
        let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
        let mut cs_ids =
            AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), master_cs_id)
                .compat()
                .try_collect::<Vec<_>>()
                .await?;
        cs_ids.reverse();

        let mapping = FilenodesOnlyPublic::default_mapping(&ctx, &repo)?;
        match FilenodesOnlyPublic::batch_derive(&ctx, &repo, cs_ids.clone(), &mapping, None).await {
            Ok(_) => {}
            Err(_) => {}
        };

        // FilenodesWrapper prevents writing of root filenode for a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157 (8th commit in repo)
        // so all children (9, 10, 11) should not have root_filenodes written
        for cs_id in cs_ids.into_iter().skip(8) {
            let filenodes = mapping.get(ctx.clone(), vec![cs_id]).await?;
            assert_eq!(filenodes.len(), 0);
        }
        Ok(())
    }

    struct FilenodesWrapper {
        inner: Arc<dyn Filenodes>,
        cs_id: HgChangesetId,
    }

    impl Filenodes for FilenodesWrapper {
        fn add_filenodes(
            &self,
            ctx: CoreContext,
            info: Vec<PreparedFilenode>,
            repo_id: RepositoryId,
        ) -> BoxFuture<FilenodeResult<()>, Error> {
            // compare PreparedFilenode::Info::Linknode
            if info
                .iter()
                .any(|filenode| filenode.info.linknode == self.cs_id)
            {
                let cs_id = self.cs_id.clone();
                return async move { Err(anyhow!(format!("filenodes for {} are prohibited", cs_id))) }
                    .boxed()
                    .compat()
                    .boxify();
            }
            self.inner.add_filenodes(ctx, info, repo_id)
        }

        fn add_or_replace_filenodes(
            &self,
            ctx: CoreContext,
            info: Vec<PreparedFilenode>,
            repo_id: RepositoryId,
        ) -> BoxFuture<FilenodeResult<()>, Error> {
            self.inner.add_or_replace_filenodes(ctx, info, repo_id)
        }

        fn get_filenode(
            &self,
            ctx: CoreContext,
            path: &RepoPath,
            filenode: HgFileNodeId,
            repo_id: RepositoryId,
        ) -> BoxFuture<FilenodeResult<Option<FilenodeInfo>>, Error> {
            self.inner.get_filenode(ctx, path, filenode, repo_id)
        }

        fn get_all_filenodes_maybe_stale(
            &self,
            ctx: CoreContext,
            path: &RepoPath,
            repo_id: RepositoryId,
            limit: Option<u64>,
        ) -> BoxFuture<FilenodeRangeResult<Vec<FilenodeInfo>>, Error> {
            self.inner
                .get_all_filenodes_maybe_stale(ctx, path, repo_id, limit)
        }

        fn prime_cache(
            &self,
            ctx: &CoreContext,
            repo_id: RepositoryId,
            filenodes: &[PreparedFilenode],
        ) {
            self.inner.prime_cache(ctx, repo_id, filenodes)
        }
    }

    async fn compare_filenodes(
        ctx: &CoreContext,
        repo: &BlobRepo,
        backup_repo: &BlobRepo,
        cs: &ChangesetId,
    ) -> Result<(), Error> {
        let prod_filenodes = repo.get_filenodes();
        let backup_filenodes = backup_repo.get_filenodes();
        let manifest = fetch_root_manifest_id(ctx, cs, repo)
            .await
            .with_context(|| format!("while fetching manifest from prod for cs {:?}", cs))?;
        cloned!(repo);
        manifest
            .list_all_entries(ctx.clone(), repo.get_blobstore())
            .map_ok(|(path, entry)| {
                cloned!(repo);
                async move {
                    let (path, node) = match (path, entry) {
                        (Some(path), Entry::Leaf((_, id))) => (RepoPath::FilePath(path), id),
                        (Some(path), Entry::Tree(id)) => (
                            RepoPath::DirectoryPath(path),
                            HgFileNodeId::new(id.into_nodehash()),
                        ),
                        (None, Entry::Leaf((_, id))) => (RepoPath::RootPath, id),
                        (None, Entry::Tree(id)) => {
                            (RepoPath::RootPath, HgFileNodeId::new(id.into_nodehash()))
                        }
                    };
                    let prod = prod_filenodes
                        .get_filenode(ctx.clone(), &path, node, repo.get_repoid())
                        .compat()
                        .await
                        .with_context(|| format!("while get prod filenode for cs {:?}", cs))?;
                    let backup = backup_filenodes
                        .get_filenode(ctx.clone(), &path, node, backup_repo.get_repoid())
                        .compat()
                        .await
                        .with_context(|| format!("while get backup filenode for cs {:?}", cs))?;
                    match (prod, backup) {
                        (FilenodeResult::Present(prod), FilenodeResult::Present(backup)) => {
                            assert!(prod == backup, "Differernt filenode for cs {} with path {:?}\nfilenode in prod repo {:?}\nfilenode in backup repo {:?}", cs, path, prod, backup);
                            Ok(())
                        }
                        (FilenodeResult::Disabled, FilenodeResult::Disabled) => Ok(()),
                        (_, _) => Err(anyhow!("filenodes results different for cs: {:?}", cs)),
                    }
                }
            })
            .try_buffer_unordered(100)
            .try_for_each(|_| async { Ok(()) })
            .await?;
        Ok(())
    }
}
