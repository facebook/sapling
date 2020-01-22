/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use context::CoreContext;
use filenodes::FilenodeInfo;
use futures::Future;
use futures_ext::FutureExt;
use futures_preview::compat::{Future01CompatExt, Stream01CompatExt};
use futures_util::{future::try_join_all, try_join, TryStreamExt};
use manifest::{find_intersection_of_diffs_and_parents, Entry};
use mercurial_types::{
    blobs::{fetch_file_envelope, File},
    fetch_manifest_envelope, HgChangesetId, HgFileEnvelope, HgFileNodeId, HgManifestEnvelope,
    HgManifestId,
};
use mononoke_types::{ChangesetId, MPath, RepoPath};

pub async fn generate_all_filenodes(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
) -> Result<Vec<FilenodeInfo>, Error> {
    let parents = repo
        .get_changeset_parents_by_bonsai(ctx.clone(), cs_id)
        .compat()
        .await?;

    // Mercurial commits can have only 2 parents, however bonsai commits can have more
    // In that case p3, p4, ... are ignored (they are called step parents)
    let cs_parents: Vec<_> = parents.into_iter().take(2).collect();
    let root_mf = fetch_root_manifest_id(ctx.clone(), cs_id, &repo);
    let parents = try_join_all(
        cs_parents
            .clone()
            .into_iter()
            .map(|p| fetch_root_manifest_id(ctx.clone(), p, &repo)),
    );

    let linknode = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
        .compat();

    let (root_mf, parents, linknode) = try_join!(root_mf, parents, linknode)?;
    let blobstore = repo.get_blobstore().boxed();
    find_intersection_of_diffs_and_parents(
        ctx.clone(),
        repo.get_blobstore(),
        root_mf,
        parents.clone(),
    )
    .compat()
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
    .map_ok(move |(path, entry)| match entry {
        Entry::Tree(hg_mf_id) => fetch_manifest_envelope(ctx.clone(), &blobstore, hg_mf_id)
            .map(move |envelope| create_manifest_filenode(path, envelope, linknode))
            .left_future()
            .compat(),
        Entry::Leaf((_, hg_filenode_id)) => {
            fetch_file_envelope(ctx.clone(), &blobstore, hg_filenode_id)
                .and_then(move |envelope| create_file_filenode(path, envelope, linknode))
                .right_future()
                .compat()
        }
    })
    .try_buffer_unordered(100)
    .try_collect()
    .await
}

fn create_manifest_filenode(
    path: Option<MPath>,
    envelope: HgManifestEnvelope,
    linknode: HgChangesetId,
) -> FilenodeInfo {
    let path = match path {
        Some(path) => RepoPath::DirectoryPath(path),
        None => RepoPath::RootPath,
    };
    let filenode = HgFileNodeId::new(envelope.node_id());
    let (p1, p2) = envelope.parents();
    let p1 = p1.map(HgFileNodeId::new);
    let p2 = p2.map(HgFileNodeId::new);

    FilenodeInfo {
        path,
        filenode,
        p1,
        p2,
        copyfrom: None,
        linknode,
    }
}

fn create_file_filenode(
    path: Option<MPath>,
    envelope: HgFileEnvelope,
    linknode: HgChangesetId,
) -> Result<FilenodeInfo, Error> {
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

    Ok(FilenodeInfo {
        path,
        filenode,
        p1,
        p2,
        copyfrom,
        linknode,
    })
}

async fn fetch_root_manifest_id(
    ctx: CoreContext,
    cs_id: ChangesetId,
    repo: &BlobRepo,
) -> Result<HgManifestId, Error> {
    let hg_cs_id = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
        .compat()
        .await?;

    let hg_cs = repo
        .get_changeset_by_changesetid(ctx.clone(), hg_cs_id)
        .compat()
        .await?;

    Ok(hg_cs.manifestid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fbinit::FacebookInit;
    use mononoke_types::FileType;
    use tests_utils::CreateCommitContext;

    async fn verify_filenodes(
        ctx: &CoreContext,
        repo: &BlobRepo,
        cs_id: ChangesetId,
        expected_paths: Vec<RepoPath>,
    ) -> Result<(), Error> {
        let filenodes = generate_all_filenodes(ctx.clone(), repo.clone(), cs_id).await?;

        assert_eq!(filenodes.len(), expected_paths.len());
        for path in expected_paths {
            assert!(filenodes
                .iter()
                .find(|filenode| filenode.path == path)
                .is_some());
        }

        let linknode = repo
            .get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
            .compat()
            .await?;

        for filenode in filenodes {
            assert_eq!(filenode.linknode, linknode);
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
        let mut runtime = tokio_compat::runtime::Runtime::new()?;
        runtime.block_on_std(test_generate_filenodes_simple(fb))
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
        let mut runtime = tokio_compat::runtime::Runtime::new()?;
        runtime.block_on_std(test_generate_filenodes_merge(fb))
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
        let mut runtime = tokio_compat::runtime::Runtime::new()?;
        runtime.block_on_std(test_generate_type_change(fb))
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

        // Root filenode was changed, and all files from p3 were added (because parents beyond
        // p1 an p2 are ignored when generating filenodes and hg changesets)
        verify_filenodes(
            &ctx,
            &repo,
            merge,
            vec![RepoPath::RootPath, RepoPath::file("path3")?],
        )
        .await?;

        Ok(())
    }

    #[fbinit::test]
    fn many_parents(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new()?;
        runtime.block_on_std(test_many_parents(fb))
    }
}
