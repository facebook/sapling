/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use borrowed::borrowed;
use context::CoreContext;
use filenodes::{FilenodeInfo, PreparedFilenode};
use futures::{future::try_join_all, pin_mut, FutureExt, StreamExt, TryFutureExt, TryStreamExt};
use futures_util::try_join;
use manifest::{find_intersection_of_diffs_and_parents, Entry};
use mercurial_types::{
    blobs::File, fetch_manifest_envelope, HgChangesetId, HgFileEnvelope, HgFileNodeId,
    HgManifestEnvelope, HgManifestId,
};
use mononoke_types::{ChangesetId, MPath, RepoPath};

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
    use crate::*;
    use derived_data::{BonsaiDerived, BonsaiDerivedMapping};
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use mononoke_types::FileType;
    use slog::info;
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
}
