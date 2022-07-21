/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::mapping::FilenodesOnlyPublic;
use crate::mapping::PreparedRootFilenode;
use anyhow::format_err;
use anyhow::Result;
use blobstore::Loadable;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use filenodes::FilenodeInfo;
use filenodes::FilenodeResult;
use filenodes::PreparedFilenode;
use futures::future::try_join_all;
use futures::pin_mut;
use futures::stream;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use itertools::Either;
use itertools::Itertools;
use manifest::find_intersection_of_diffs_and_parents;
use manifest::Entry;
use mercurial_derived_data::MappedHgChangesetId;
use mercurial_types::blobs::File;
use mercurial_types::fetch_manifest_envelope;
use mercurial_types::nodehash::NULL_HASH;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileEnvelope;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestEnvelope;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use mononoke_types::RepoPath;

pub async fn derive_filenodes(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bcs: BonsaiChangeset,
) -> Result<FilenodesOnlyPublic> {
    if tunables::tunables().get_filenodes_disabled() {
        return Ok(FilenodesOnlyPublic::Disabled);
    }
    let (_, public_filenode, non_roots) =
        prepare_filenodes_for_cs(ctx, derivation_ctx, bcs).await?;
    if !non_roots.is_empty() {
        if let FilenodeResult::Disabled = derivation_ctx
            .filenodes()?
            .add_filenodes(ctx, non_roots)
            .await?
        {
            return Ok(FilenodesOnlyPublic::Disabled);
        }
    }
    // In case it got updated while deriving
    if tunables::tunables().get_filenodes_disabled() {
        return Ok(FilenodesOnlyPublic::Disabled);
    }
    Ok(public_filenode)
}

pub async fn derive_filenodes_in_batch(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    batch: Vec<BonsaiChangeset>,
) -> Result<Vec<(ChangesetId, FilenodesOnlyPublic, Vec<PreparedFilenode>)>> {
    stream::iter(
        batch
            .clone()
            .into_iter()
            .map(|bcs| async move { prepare_filenodes_for_cs(ctx, derivation_ctx, bcs).await }),
    )
    .buffered(100)
    .try_collect()
    .await
}

pub async fn prepare_filenodes_for_cs(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bcs: BonsaiChangeset,
) -> Result<(ChangesetId, FilenodesOnlyPublic, Vec<PreparedFilenode>)> {
    let filenodes = generate_all_filenodes(ctx, derivation_ctx, &bcs).await?;
    if filenodes.is_empty() {
        // This commit didn't create any new filenodes, and it's root manifest is the
        // same as one of the parents (that can happen if this commit is empty).
        // In that case we don't need to insert a root filenode - it will be inserted
        // when parent is derived.
        Ok((
            bcs.get_changeset_id(),
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
                bcs.get_changeset_id(),
                FilenodesOnlyPublic::Present {
                    root_filenode: Some(root_filenode),
                },
                non_roots,
            )),
            _ => Err(format_err!(
                "expected exactly one root, found {} for cs_id {}",
                roots_num,
                bcs.get_changeset_id().to_string()
            )),
        }
    }
}

pub async fn generate_all_filenodes(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bcs: &BonsaiChangeset,
) -> Result<Vec<PreparedFilenode>> {
    let blobstore = derivation_ctx.blobstore();
    let hg_id = derivation_ctx
        .derive_dependency::<MappedHgChangesetId>(ctx, bcs.get_changeset_id())
        .await?
        .hg_changeset_id();
    let root_mf = hg_id.load(ctx, &blobstore).await?.manifestid();
    // In case of non-existant manifest (that's created by hg if the first commit in the repo
    // is empty) it's fine to return the empty list of filenodes.
    if root_mf.clone().into_nodehash() == NULL_HASH {
        return Ok(vec![]);
    }
    // Bonsai might have > 2 parents, while mercurial supports at most 2.
    // That's fine for us - we just won't generate filenodes for paths that came from
    // stepparents. That means that linknode for these filenodes will point to a stepparent
    let parents = try_join_all(
        derivation_ctx
            .fetch_parents::<MappedHgChangesetId>(ctx, bcs)
            .await?
            .into_iter()
            .map(|id| async move {
                Result::<_>::Ok(
                    id.hg_changeset_id()
                        .load(ctx, &blobstore)
                        .await?
                        .manifestid(),
                )
            }),
    )
    .await?;
    let linknode = hg_id;

    (async_stream::stream! {
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
) -> Result<PreparedFilenode> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use anyhow::Context;
    use anyhow::Result;
    use async_trait::async_trait;
    use blobrepo::BlobRepo;
    use cloned::cloned;
    use derived_data_manager::BatchDeriveOptions;
    use fbinit::FacebookInit;
    use filenodes::FilenodeRangeResult;
    use filenodes::Filenodes;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use futures::compat::Stream01CompatExt;
    use manifest::ManifestOps;
    use maplit::hashmap;
    use mercurial_derived_data::DeriveHgChangeset;
    use mononoke_types::FileType;
    use repo_derived_data::RepoDerivedDataRef;
    use revset::AncestorsNodeStream;
    use slog::info;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::resolve_cs_id;
    use tests_utils::CreateCommitContext;
    use tunables::with_tunables;
    use tunables::MononokeTunables;

    async fn verify_filenodes(
        ctx: &CoreContext,
        repo: &BlobRepo,
        cs_id: ChangesetId,
        expected_paths: Vec<RepoPath>,
    ) -> Result<()> {
        let bonsai = cs_id.load(ctx, repo.blobstore()).await?;
        let filenodes = generate_all_filenodes(
            ctx,
            &repo.repo_derived_data().manager().derivation_context(None),
            &bonsai,
        )
        .await?;

        assert_eq!(filenodes.len(), expected_paths.len());
        for path in expected_paths {
            assert!(filenodes.iter().any(|filenode| filenode.path == path));
        }

        let linknode = repo.derive_hg_changeset(ctx, cs_id).await?;

        for filenode in filenodes {
            assert_eq!(filenode.info.linknode, linknode);
        }
        Ok(())
    }

    async fn test_generate_filenodes_simple(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
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
    fn generate_filenodes_simple(fb: FacebookInit) -> Result<()> {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_generate_filenodes_simple(fb))
    }

    async fn test_generate_filenodes_merge(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
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
    fn generate_filenodes_merge(fb: FacebookInit) -> Result<()> {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_generate_filenodes_merge(fb))
    }

    async fn test_generate_type_change(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
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
    fn generate_filenodes_type_change(fb: FacebookInit) -> Result<()> {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_generate_type_change(fb))
    }

    async fn test_many_parents(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
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
    fn many_parents(fb: FacebookInit) -> Result<()> {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_many_parents(fb))
    }

    async fn test_derive_empty_commits(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
        let parent_empty = CreateCommitContext::new_root(&ctx, &repo).commit().await?;

        let child_empty = CreateCommitContext::new(&ctx, &repo, vec![parent_empty])
            .add_file("file", "content")
            .commit()
            .await?;

        let manager = repo.repo_derived_data().manager();

        manager
            .derive::<FilenodesOnlyPublic>(&ctx, child_empty, None)
            .await?;

        // Make sure they are in the mapping
        let maps = manager
            .fetch_derived_batch::<FilenodesOnlyPublic>(&ctx, vec![parent_empty, child_empty], None)
            .await?;

        assert_eq!(maps.len(), 2);
        Ok(())
    }

    #[fbinit::test]
    fn derive_empty_commits(fb: FacebookInit) -> Result<()> {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_derive_empty_commits(fb))
    }

    async fn test_derive_only_empty_commits(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;

        let parent_empty = CreateCommitContext::new_root(&ctx, &repo).commit().await?;
        let child_empty = CreateCommitContext::new(&ctx, &repo, vec![parent_empty])
            .commit()
            .await?;

        let manager = repo.repo_derived_data().manager();
        manager
            .derive::<FilenodesOnlyPublic>(&ctx, child_empty, None)
            .await?;

        // Make sure they are in the mapping
        let maps = manager
            .fetch_derived_batch::<FilenodesOnlyPublic>(&ctx, vec![child_empty, parent_empty], None)
            .await?;
        assert_eq!(maps.len(), 2);
        Ok(())
    }

    #[fbinit::test]
    fn derive_only_empty_commits(fb: FacebookInit) -> Result<()> {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_derive_only_empty_commits(fb))
    }

    #[fbinit::test]
    fn derive_disabled_filenodes(fb: FacebookInit) -> Result<()> {
        let tunables = MononokeTunables::default();
        tunables.update_bools(&hashmap! {"filenodes_disabled".to_string() => true});

        with_tunables(tunables, || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()?;
            runtime.block_on(test_derive_disabled_filenodes(fb))
        })
    }

    async fn test_derive_disabled_filenodes(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
        let cs = CreateCommitContext::new_root(&ctx, &repo).commit().await?;
        let derived = repo
            .repo_derived_data()
            .derive::<FilenodesOnlyPublic>(&ctx, cs)
            .await?;
        assert_eq!(derived, FilenodesOnlyPublic::Disabled);

        assert_eq!(
            repo.repo_derived_data()
                .fetch_derived::<FilenodesOnlyPublic>(&ctx, cs)
                .await?
                .unwrap(),
            FilenodesOnlyPublic::Disabled
        );

        Ok(())
    }

    #[fbinit::test]
    async fn verify_batch_and_sequential_derive(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo1 = Linear::getrepo(fb).await;
        let repo2 = Linear::getrepo(fb).await;
        let master_cs_id = resolve_cs_id(&ctx, &repo1, "master").await?;
        let mut cs_ids =
            AncestorsNodeStream::new(ctx.clone(), &repo1.get_changeset_fetcher(), master_cs_id)
                .compat()
                .try_collect::<Vec<_>>()
                .await?;
        cs_ids.reverse();

        let manager1 = repo1.repo_derived_data().manager();
        manager1
            .backfill_batch::<FilenodesOnlyPublic>(
                &ctx,
                cs_ids.clone(),
                BatchDeriveOptions::Parallel { gap_size: None },
                None,
            )
            .await?;
        let batch = manager1
            .fetch_derived_batch::<FilenodesOnlyPublic>(&ctx, cs_ids.clone(), None)
            .await?;

        let sequential = {
            let mut res = HashMap::new();
            for cs in cs_ids.clone() {
                let root_filenode = repo2
                    .repo_derived_data()
                    .derive::<FilenodesOnlyPublic>(&ctx, cs)
                    .await?;
                res.insert(cs, root_filenode);
            }
            res
        };

        assert_eq!(batch, sequential);
        for cs in cs_ids {
            compare_filenodes(&ctx, &repo1, &repo2, cs).await?;
        }
        Ok(())
    }

    #[fbinit::test]
    async fn derive_parents_before_children(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let filenodes_cs_id = Arc::new(Mutex::new(None));
        let mut factory = TestRepoFactory::new(fb)?;
        let repo: BlobRepo = factory
            .with_filenodes_override({
                cloned!(filenodes_cs_id);
                move |inner| {
                    Arc::new(FilenodesWrapper {
                        inner,
                        cs_id: filenodes_cs_id.clone(),
                    })
                }
            })
            .build()?;
        Linear::initrepo(fb, &repo).await;
        let commit8 =
            resolve_cs_id(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await?;
        let commit8 = repo.derive_hg_changeset(&ctx, commit8).await?;
        *filenodes_cs_id.lock().unwrap() = Some(commit8);
        let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
        let mut cs_ids =
            AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), master_cs_id)
                .compat()
                .try_collect::<Vec<_>>()
                .await?;
        cs_ids.reverse();

        let manager = repo.repo_derived_data().manager();

        match manager
            .backfill_batch::<FilenodesOnlyPublic>(
                &ctx,
                cs_ids.clone(),
                BatchDeriveOptions::Parallel { gap_size: None },
                None,
            )
            .await
        {
            Ok(_) => {}
            Err(_) => {}
        };

        // FilenodesWrapper prevents writing of root filenode for a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157 (8th commit in repo)
        // so all children (9, 10, 11) should not have root_filenodes written
        for cs_id in cs_ids.into_iter().skip(8) {
            let filenode = manager
                .fetch_derived::<FilenodesOnlyPublic>(&ctx, cs_id, None)
                .await?;
            assert_eq!(filenode, None);
        }
        Ok(())
    }

    struct FilenodesWrapper {
        inner: Arc<dyn Filenodes + Send + Sync>,
        cs_id: Arc<Mutex<Option<HgChangesetId>>>,
    }

    #[async_trait]
    impl Filenodes for FilenodesWrapper {
        async fn add_filenodes(
            &self,
            ctx: &CoreContext,
            info: Vec<PreparedFilenode>,
        ) -> Result<FilenodeResult<()>> {
            // compare PreparedFilenode::Info::Linknode
            if let Some(cs_id) = *self.cs_id.lock().unwrap() {
                if info.iter().any(|filenode| filenode.info.linknode == cs_id) {
                    return Err(anyhow!("filenodes for {} are prohibited", cs_id));
                }
            }
            self.inner.add_filenodes(ctx, info).await
        }

        async fn add_or_replace_filenodes(
            &self,
            ctx: &CoreContext,
            info: Vec<PreparedFilenode>,
        ) -> Result<FilenodeResult<()>> {
            self.inner.add_or_replace_filenodes(ctx, info).await
        }

        async fn get_filenode(
            &self,
            ctx: &CoreContext,
            path: &RepoPath,
            filenode: HgFileNodeId,
        ) -> Result<FilenodeResult<Option<FilenodeInfo>>> {
            self.inner.get_filenode(ctx, path, filenode).await
        }

        async fn get_all_filenodes_maybe_stale(
            &self,
            ctx: &CoreContext,
            path: &RepoPath,
            limit: Option<u64>,
        ) -> Result<FilenodeRangeResult<Vec<FilenodeInfo>>> {
            self.inner
                .get_all_filenodes_maybe_stale(ctx, path, limit)
                .await
        }

        fn prime_cache(&self, ctx: &CoreContext, filenodes: &[PreparedFilenode]) {
            self.inner.prime_cache(ctx, filenodes)
        }
    }

    async fn compare_filenodes(
        ctx: &CoreContext,
        repo: &BlobRepo,
        backup_repo: &BlobRepo,
        cs: ChangesetId,
    ) -> Result<()> {
        let manifest = repo
            .derive_hg_changeset(ctx, cs)
            .await?
            .load(ctx, repo.blobstore())
            .await
            .with_context(|| format!("while fetching manifest from prod for cs {:?}", cs))?
            .manifestid();
        manifest
            .list_all_entries(ctx.clone(), repo.get_blobstore())
            .map_ok(|(path, entry)| {
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
                    let prod = repo.filenodes()
                        .get_filenode(ctx, &path, node)
                        .await
                        .with_context(|| format!("while get prod filenode for cs {:?}", cs))?;
                    let backup = backup_repo.filenodes()
                        .get_filenode(ctx, &path, node)
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
