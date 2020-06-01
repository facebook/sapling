/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobstore::{Blobstore, Loadable};
use cloned::cloned;
use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    future::{self as new_future, FutureExt as NewFutureExt, TryFutureExt},
};
use futures_ext::{BoxFuture, FutureExt};
use futures_old::{future, sync::mpsc, Future};
use manifest::{derive_manifest_with_io_sender, Entry, LeafInfo, TreeInfo};
use metaconfig_types::UnodeVersion;
use mononoke_types::unode::{FileUnode, ManifestUnode, UnodeEntry};
use mononoke_types::{
    BlobstoreValue, ChangesetId, ContentId, FileType, FileUnodeId, MPath, MPathElement, MPathHash,
    ManifestUnodeId, MononokeId,
};
use repo_blobstore::RepoBlobstore;
use std::collections::BTreeMap;

use crate::ErrorKind;

/// Derives unode manifests for bonsai changeset `cs_id` given parent unode manifests.
/// Note that `derive_manifest()` does a lot of the heavy lifting for us, and this crate has to
/// provide only functions to create a single unode file or single unode tree (
/// `create_unode_manifest` and `create_unode_file`).
pub(crate) fn derive_unode_manifest(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
    parents: Vec<ManifestUnodeId>,
    changes: Vec<(MPath, Option<(ContentId, FileType)>)>,
) -> impl Future<Item = ManifestUnodeId, Error = Error> {
    let unode_version = repo.get_derived_data_config().unode_version;

    future::lazy(move || {
        let parents: Vec<_> = parents.into_iter().collect();
        let blobstore = repo.get_blobstore();

        derive_manifest_with_io_sender(
            ctx.clone(),
            repo.get_blobstore(),
            parents.clone(),
            changes,
            {
                cloned!(blobstore, ctx, cs_id);
                move |tree_info, sender| {
                    create_unode_manifest(
                        ctx.clone(),
                        cs_id,
                        blobstore.clone(),
                        Some(sender),
                        tree_info,
                        unode_version,
                    )
                }
            },
            {
                cloned!(blobstore, ctx, cs_id);
                move |leaf_info, sender| {
                    create_unode_file(
                        ctx.clone(),
                        cs_id,
                        blobstore.clone(),
                        sender,
                        leaf_info,
                        unode_version,
                    )
                }
            },
        )
        .and_then({
            cloned!(ctx, blobstore);
            move |maybe_tree_id| match maybe_tree_id {
                Some(tree_id) => future::ok(tree_id).left_future(),
                None => {
                    // All files have been deleted, generate empty **root** manifest
                    let tree_info = TreeInfo {
                        path: None,
                        parents,
                        subentries: Default::default(),
                    };
                    create_unode_manifest(ctx, cs_id, blobstore, None, tree_info, unode_version)
                        .map(|((), tree_id)| tree_id)
                        .right_future()
                }
            }
        })
    })
}

// Note that in some rare cases it's possible to have unode where one parent is ancestor of another
// (that applies both to files and directories)
//
//  Example:
//       4 o <- merge commit modifies file 'A'
//        / \
//     2 o ---> changed file 'A' content to 'B'
//       |   |
//       | 3 o -> changed some other file
//       \  /
//      1 o  <- created file 'A' with content 'A'
//
// In that case unode for file 'A' in a merge commit will have two parents - from commit '2' and
// from commit '1', and unode from commit '1' is ancestor of unode from commit '2'.
// Case like that might create slight confusion, however it should be rare and we should be
// able to fix it in the ui.
fn create_unode_manifest(
    ctx: CoreContext,
    linknode: ChangesetId,
    blobstore: RepoBlobstore,
    sender: Option<mpsc::UnboundedSender<BoxFuture<(), Error>>>,
    tree_info: TreeInfo<ManifestUnodeId, FileUnodeId, ()>,
    unode_version: UnodeVersion,
) -> impl Future<Item = ((), ManifestUnodeId), Error = Error> {
    async move {
        let mut subentries = BTreeMap::new();
        for (basename, (_context, entry)) in tree_info.subentries {
            match entry {
                Entry::Tree(mf_unode) => {
                    subentries.insert(basename, UnodeEntry::Directory(mf_unode));
                }
                Entry::Leaf(file_unode) => {
                    subentries.insert(basename, UnodeEntry::File(file_unode));
                }
            }
        }
        if unode_version == UnodeVersion::V2 && tree_info.parents.len() > 1 {
            if let Some(mf_unode_id) =
                reuse_manifest_parent(&ctx, &blobstore, &tree_info.parents, &subentries).await?
            {
                return Ok(((), mf_unode_id));
            }
        }

        let mf_unode = ManifestUnode::new(tree_info.parents, subentries, linknode);
        let mf_unode_id = mf_unode.get_unode_id();

        let key = mf_unode_id.blobstore_key();
        let blob = mf_unode.into_blob();
        let f = blobstore.put(ctx, key, blob.into());

        match sender {
            Some(sender) => sender
                .unbounded_send(f)
                .map_err(|err| format_err!("failed to send manifest future {}", err))?,
            None => f.compat().await?,
        };
        Ok(((), mf_unode_id))
    }
    .boxed()
    .compat()
}

fn create_unode_file(
    ctx: CoreContext,
    linknode: ChangesetId,
    blobstore: RepoBlobstore,
    sender: mpsc::UnboundedSender<BoxFuture<(), Error>>,
    leaf_info: LeafInfo<FileUnodeId, (ContentId, FileType)>,
    unode_version: UnodeVersion,
) -> impl Future<Item = ((), FileUnodeId), Error = Error> {
    fn save_unode(
        ctx: CoreContext,
        blobstore: RepoBlobstore,
        sender: mpsc::UnboundedSender<BoxFuture<(), Error>>,
        parents: Vec<FileUnodeId>,
        content_id: ContentId,
        file_type: FileType,
        path_hash: MPathHash,
        linknode: ChangesetId,
        unode_version: UnodeVersion,
    ) -> BoxFuture<FileUnodeId, Error> {
        async move {
            if unode_version == UnodeVersion::V2 && parents.len() > 1 {
                if let Some(parent) =
                    reuse_file_parent(&ctx, &blobstore, &parents, &content_id, file_type).await?
                {
                    return Ok(parent);
                }
            };
            let file_unode = FileUnode::new(parents, content_id, file_type, path_hash, linknode);
            let file_unode_id = file_unode.get_unode_id();

            let f = future::lazy(move || {
                blobstore.put(
                    ctx,
                    file_unode_id.blobstore_key(),
                    file_unode.into_blob().into(),
                )
            })
            .boxify();

            sender
                .unbounded_send(f)
                .map_err(|err| format_err!("failed to send manifest future {}", err))?;
            Ok(file_unode_id)
        }
        .boxed()
        .compat()
        .boxify()
    }

    let LeafInfo {
        leaf,
        path,
        parents,
    } = leaf_info;

    if let Some((content_id, file_type)) = leaf {
        save_unode(
            ctx,
            blobstore,
            sender,
            parents,
            content_id,
            file_type,
            path.get_path_hash(),
            linknode,
            unode_version,
        )
    } else {
        // We can end up in this codepath if there are at least 2 parent commits have a unode with
        // this file path, and these unodes are different, but current bonsai changeset have no
        // changes for this file path.
        //
        //  Example:
        //         o <- merge commit, it doesn't modify any of the files
        //        / \
        //       o ---> changed file 'A' content to 'B'
        //       |  |
        //       |   o -> changed file 'A content to 'B' as well
        //       \  /
        //        o  <- created file 'A' with content 'A'
        //
        // In that case we need to check file content and file type.
        // if they are the same then we need to create a new file unode.
        //
        // If content or file type are different then we need to return an error

        // Note that there's a difference from how we handle this case in mercurial manifests.
        // In mercurial manifests we compare file content with copy information, while in unodes
        // copy information is ignored. It might mean that some bonsai changesets would be
        // considered valid for unode manifests, but invalid for mercurial
        if parents.len() < 2 {
            return future::err(
                ErrorKind::InvalidBonsai(
                    "no change is provided, but file unode has only one parent".to_string(),
                )
                .into(),
            )
            .boxify();
        }
        future::join_all(parents.clone().into_iter().map({
            cloned!(blobstore, ctx);
            move |id| id.load(ctx.clone(), &blobstore.clone())
        }))
        .from_err()
        .and_then(
            move |parent_unodes| match return_if_unique_filenode(&parent_unodes) {
                Some((content_id, file_type)) => save_unode(
                    ctx,
                    blobstore,
                    sender,
                    parents,
                    content_id.clone(),
                    *file_type,
                    path.get_path_hash(),
                    linknode,
                    unode_version,
                ),
                _ => future::err(
                    ErrorKind::InvalidBonsai(
                        "no change is provided, but content is different".to_string(),
                    )
                    .into(),
                )
                .boxify(),
            },
        )
        .boxify()
    }
    .map(|leaf_id| ((), leaf_id))
    .boxify()
}

// reuse_manifest_parent() and reuse_file_parent() are used in unodes v2 in order to avoid
// creating unnecessary unodes that were created in unodes v1. Consider this graph with a diamond merge:
//
//   O <- merge commit, doesn't change anything
//  / \
// P1  |  <- modified "file.txt"
// |   P2    <- created "other.txt"
// \  /
//  ROOT <- created "file.txt"
//
// In unodes v1 merge commit would contain a unode for "file.txt" which points to "file.txt"
// unode from P1 and and "file.txt" unode from ROOT. This is
// actually quite unexpected - merge commit didn't touch "file.txt" at all, but "file.txt"'s history
// would contain a merge commit.
//
// reuse_manifest_parent() and reuse_file_parent() are heuristics that help fix this problem.
// They check if content of the unode in the merge commit is the same as the content of unode
// in one of the parents. If yes, then this unode is reused. Note - it actually might also lead
// to surprising results in some cases:
//
//   O <- merge commit, doesn't change anything
//  / \
// P1  |  <- modified "file.txt" to "B"
// |   P2    <- modified "file.txt" to "B"
// \  /
//  ROOT <- created "file.txt" with content "A"
//
// In that case it would be ideal to preserve that "file.txt" was modified in both commits.
// But we consider this case is rare enough to not worry about it.

async fn reuse_manifest_parent(
    ctx: &CoreContext,
    blobstore: &RepoBlobstore,
    parents: &Vec<ManifestUnodeId>,
    subentries: &BTreeMap<MPathElement, UnodeEntry>,
) -> Result<Option<ManifestUnodeId>, Error> {
    let parents = new_future::try_join_all(parents.into_iter().map(|id| {
        id.load(ctx.clone(), blobstore)
            .map_err(Error::from)
            .compat()
    }))
    .await?;

    if let Some(mf_unode) = parents.iter().find(|p| p.subentries() == subentries) {
        Ok(Some(mf_unode.get_unode_id()))
    } else {
        Ok(None)
    }
}

async fn reuse_file_parent(
    ctx: &CoreContext,
    blobstore: &RepoBlobstore,
    parents: &Vec<FileUnodeId>,
    content_id: &ContentId,
    file_type: FileType,
) -> Result<Option<FileUnodeId>, Error> {
    let parents = new_future::try_join_all(parents.into_iter().map(|id| {
        id.load(ctx.clone(), blobstore)
            .map_err(Error::from)
            .compat()
    }))
    .await?;

    if let Some(file_unode) = parents
        .iter()
        .find(|p| p.content_id() == content_id && p.file_type() == &file_type)
    {
        Ok(Some(file_unode.get_unode_id()))
    } else {
        Ok(None)
    }
}

// If all elements in `unodes` are the same than this element is returned, otherwise None is returned
fn return_if_unique_filenode(unodes: &Vec<FileUnode>) -> Option<(&ContentId, &FileType)> {
    let mut iter = unodes
        .iter()
        .map(|elem| (elem.content_id(), elem.file_type()));
    let first_elem = iter.next()?;
    if iter.all(|next_elem| next_elem == first_elem) {
        Some(first_elem)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapping::{get_file_changes, RootUnodeManifestId};
    use anyhow::Result;
    use blobrepo::save_bonsai_changesets;
    use blobrepo::{BlobRepo, DangerousOverride};
    use blobrepo_factory::new_memblob_empty;
    use blobstore::Storable;
    use bytes::Bytes;
    use derived_data::BonsaiDerived;
    use derived_data_filenodes::FilenodesOnlyPublic;
    use fbinit::FacebookInit;
    use fixtures::linear;
    use futures::{compat::Future01CompatExt, FutureExt as NewFutureExt, TryFutureExt};
    use futures_old::Stream;
    use manifest::ManifestOps;
    use maplit::btreemap;
    use mercurial_types::{blobs::BlobManifest, HgFileNodeId, HgManifestId};
    use metaconfig_types::DerivedDataConfig;
    use mononoke_types::{
        BlobstoreValue, BonsaiChangeset, BonsaiChangesetMut, DateTime, FileChange, FileContents,
        RepoPath,
    };
    use std::collections::{HashSet, VecDeque};
    use test_utils::{get_bonsai_changeset, iterate_all_entries};
    use tests_utils::{resolve_cs_id, CreateCommitContext};
    use tokio_compat::runtime::Runtime;

    #[fbinit::test]
    fn linear_test(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = Runtime::new().unwrap();
        let repo = runtime.block_on_std(linear::getrepo(fb));
        let ctx = CoreContext::test_mock(fb);

        // Derive filenodes because they are going to be used in this test
        runtime.block_on({
            cloned!(ctx, repo);
            async move {
                let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
                FilenodesOnlyPublic::derive(ctx, repo, master_cs_id)
                    .compat()
                    .await?;
                let res: Result<(), Error> = Ok(());
                res
            }
            .boxed()
            .compat()
        })?;
        let parent_unode_id = {
            let parent_hg_cs = "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536";
            let (bcs_id, bcs) =
                get_bonsai_changeset(ctx.clone(), repo.clone(), &mut runtime, parent_hg_cs);

            let f = derive_unode_manifest(
                ctx.clone(),
                repo.clone(),
                bcs_id,
                vec![],
                get_file_changes(&bcs),
            );

            let unode_id = runtime.block_on(f).unwrap();
            // Make sure it's saved in the blobstore
            runtime
                .block_on(unode_id.load(ctx.clone(), repo.blobstore()))
                .unwrap();
            let all_unodes = runtime
                .block_on(
                    iterate_all_entries(ctx.clone(), repo.clone(), Entry::Tree(unode_id)).collect(),
                )
                .unwrap();
            let mut paths: Vec<_> = all_unodes.into_iter().map(|(path, _)| path).collect();
            paths.sort();
            assert_eq!(
                paths,
                vec![
                    None,
                    Some(MPath::new("1").unwrap()),
                    Some(MPath::new("files").unwrap())
                ]
            );
            unode_id
        };

        {
            let child_hg_cs = "3e0e761030db6e479a7fb58b12881883f9f8c63f";
            let (bcs_id, bcs) =
                get_bonsai_changeset(ctx.clone(), repo.clone(), &mut runtime, child_hg_cs);

            let f = derive_unode_manifest(
                ctx.clone(),
                repo.clone(),
                bcs_id,
                vec![parent_unode_id.clone()],
                get_file_changes(&bcs),
            );

            let unode_id = runtime.block_on(f).unwrap();
            // Make sure it's saved in the blobstore
            let root_unode: Result<_> =
                runtime.block_on(unode_id.load(ctx.clone(), repo.blobstore()).from_err());
            let root_unode = root_unode.unwrap();
            assert_eq!(root_unode.parents(), &vec![parent_unode_id]);

            let root_filenode_id = fetch_root_filenode_id(fb, &mut runtime, repo.clone(), bcs_id);
            assert_eq!(
                find_unode_history(
                    fb,
                    &mut runtime,
                    repo.clone(),
                    UnodeEntry::Directory(unode_id)
                ),
                find_filenode_history(fb, &mut runtime, repo.clone(), root_filenode_id),
            );

            let all_unodes = runtime
                .block_on(
                    iterate_all_entries(ctx.clone(), repo.clone(), Entry::Tree(unode_id)).collect(),
                )
                .unwrap();
            let mut paths: Vec<_> = all_unodes.into_iter().map(|(path, _)| path).collect();
            paths.sort();
            assert_eq!(
                paths,
                vec![
                    None,
                    Some(MPath::new("1").unwrap()),
                    Some(MPath::new("2").unwrap()),
                    Some(MPath::new("files").unwrap())
                ]
            );
        }
        Ok(())
    }

    #[fbinit::test]
    fn test_same_content_different_paths(fb: FacebookInit) {
        let mut runtime = Runtime::new().unwrap();
        let repo = runtime.block_on_std(linear::getrepo(fb));
        let ctx = CoreContext::test_mock(fb);

        fn check_unode_uniqeness(
            ctx: CoreContext,
            repo: BlobRepo,
            runtime: &mut Runtime,
            file_changes: BTreeMap<MPath, Option<FileChange>>,
        ) {
            let bcs = create_bonsai_changeset(ctx.fb, repo.clone(), runtime, file_changes);
            let bcs_id = bcs.get_changeset_id();

            let f = derive_unode_manifest(
                ctx.clone(),
                repo.clone(),
                bcs_id,
                vec![],
                get_file_changes(&bcs),
            );
            let unode_id = runtime.block_on(f).unwrap();

            let unode_mf: Result<_> =
                runtime.block_on(unode_id.load(ctx.clone(), repo.blobstore()).from_err());
            let unode_mf = unode_mf.unwrap();

            // Unodes should be unique even if content is the same. Check it
            let vals: Vec<_> = unode_mf.list().collect();
            assert_eq!(vals.len(), 2);
            assert_ne!(vals.get(0), vals.get(1));
        }

        let file_changes = store_files(
            ctx.clone(),
            &mut runtime,
            btreemap! {"file1" => Some(("content", FileType::Regular)), "file2" => Some(("content", FileType::Regular))},
            repo.clone(),
        );
        check_unode_uniqeness(ctx.clone(), repo.clone(), &mut runtime, file_changes);

        let file_changes = store_files(
            ctx.clone(),
            &mut runtime,
            btreemap! {"dir1/file" => Some(("content", FileType::Regular)), "dir2/file" => Some(("content", FileType::Regular))},
            repo.clone(),
        );
        check_unode_uniqeness(ctx.clone(), repo.clone(), &mut runtime, file_changes);
    }

    #[fbinit::test]
    fn test_same_content_no_change(fb: FacebookInit) {
        let mut runtime = Runtime::new().unwrap();
        let repo = runtime.block_on_std(linear::getrepo(fb));
        let ctx = CoreContext::test_mock(fb);

        assert!(build_diamond_graph(
            ctx.clone(),
            &mut runtime,
            repo.clone(),
            btreemap! {"A" => Some(("A", FileType::Regular))},
            btreemap! {"A" => Some(("B", FileType::Regular))},
            btreemap! {"A" => Some(("B", FileType::Regular))},
            btreemap! {},
        )
        .is_ok());

        // Content is different - fail!
        assert!(build_diamond_graph(
            ctx.clone(),
            &mut runtime,
            repo.clone(),
            btreemap! {"A" => Some(("A", FileType::Regular))},
            btreemap! {"A" => Some(("B", FileType::Regular))},
            btreemap! {"A" => Some(("C", FileType::Regular))},
            btreemap! {},
        )
        .is_err());

        // Type is different - fail!
        assert!(build_diamond_graph(
            ctx,
            &mut runtime,
            repo,
            btreemap! {"A" => Some(("A", FileType::Regular))},
            btreemap! {"A" => Some(("B", FileType::Regular))},
            btreemap! {"A" => Some(("B", FileType::Executable))},
            btreemap! {},
        )
        .is_err());
    }

    async fn diamond_merge_unodes_v2(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = new_memblob_empty(None)?;
        let merged_files = "dir/file.txt";
        let root_commit = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(merged_files, "a")
            .commit()
            .await?;

        let p1 = CreateCommitContext::new(&ctx, &repo, vec![root_commit])
            .add_file("p1.txt", "p1")
            .add_file(merged_files, "b")
            .commit()
            .await?;

        let p2 = CreateCommitContext::new(&ctx, &repo, vec![root_commit])
            .add_file("p2.txt", "p2")
            .commit()
            .await?;

        let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2])
            .add_file(merged_files, "b")
            .commit()
            .await?;

        let find_unodes = {
            |ctx: CoreContext, repo: BlobRepo| async move {
                let p1_root_unode_mf_id =
                    RootUnodeManifestId::derive(ctx.clone(), repo.clone(), p1)
                        .compat()
                        .await?;

                let mut p1_unodes = p1_root_unode_mf_id
                    .manifest_unode_id()
                    .find_entries(
                        ctx.clone(),
                        repo.get_blobstore(),
                        vec![Some(MPath::new(&merged_files)?), Some(MPath::new("dir")?)],
                        // Some(MPath::new(&merged_files)?),
                    )
                    .collect()
                    .compat()
                    .await?;
                p1_unodes.sort_by_key(|(path, _)| path.clone());

                let merge_root_unode_mf_id =
                    RootUnodeManifestId::derive(ctx.clone(), repo.clone(), merge)
                        .compat()
                        .await?;

                let mut merge_unodes = merge_root_unode_mf_id
                    .manifest_unode_id()
                    .find_entries(
                        ctx.clone(),
                        repo.get_blobstore(),
                        vec![Some(MPath::new(&merged_files)?), Some(MPath::new("dir")?)],
                    )
                    .collect()
                    .compat()
                    .await?;
                merge_unodes.sort_by_key(|(path, _)| path.clone());

                let res: Result<_, Error> = Ok((p1_unodes, merge_unodes));
                res
            }
        };

        // Unodes v2 should just reuse merged filenode
        let (p1_unodes, merge_unodes) = find_unodes(ctx.clone(), repo.clone()).await?;
        assert_eq!(p1_unodes, merge_unodes);

        // Unodes v1 should create a new one that points to the parent unode
        let repo = repo.dangerous_override(|mut derived_data_config: DerivedDataConfig| {
            derived_data_config.unode_version = UnodeVersion::V1;
            derived_data_config
        });
        let (p1_unodes, merge_unodes) = find_unodes(ctx.clone(), repo.clone()).await?;
        assert_ne!(p1_unodes, merge_unodes);

        for ((_, p1), (_, merge)) in p1_unodes.iter().zip(merge_unodes.iter()) {
            let merge_unode = merge
                .load(ctx.clone(), &repo.get_blobstore())
                .compat()
                .await?;

            match (p1, merge_unode) {
                (Entry::Leaf(p1), Entry::Leaf(ref merge_unode)) => {
                    assert!(merge_unode.parents().contains(p1));
                }
                (Entry::Tree(p1), Entry::Tree(ref merge_unode)) => {
                    assert!(merge_unode.parents().contains(p1));
                }
                _ => {
                    return Err(format_err!("inconsistent unodes in p1 and merge"));
                }
            }
        }

        Ok(())
    }

    #[fbinit::test]
    fn test_diamond_merge_unodes_v2(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = Runtime::new()?;
        runtime.block_on_std(diamond_merge_unodes_v2(fb))
    }

    #[fbinit::test]
    fn test_parent_order(fb: FacebookInit) {
        let repo = new_memblob_empty(None).unwrap();
        let mut runtime = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let p1_root_unode_id = create_changeset_and_derive_unode(
            ctx.clone(),
            repo.clone(),
            &mut runtime,
            btreemap! {"A" => Some(("A", FileType::Regular))},
        );

        let p2_root_unode_id = create_changeset_and_derive_unode(
            ctx.clone(),
            repo.clone(),
            &mut runtime,
            btreemap! {"A" => Some(("B", FileType::Regular))},
        );

        let file_changes = store_files(
            ctx.clone(),
            &mut runtime,
            btreemap! { "A" => Some(("C", FileType::Regular)) },
            repo.clone(),
        );
        let bcs = create_bonsai_changeset(fb, repo.clone(), &mut runtime, file_changes);
        let bcs_id = bcs.get_changeset_id();

        let f = derive_unode_manifest(
            ctx.clone(),
            repo.clone(),
            bcs_id,
            vec![p1_root_unode_id, p2_root_unode_id],
            get_file_changes(&bcs),
        );
        let root_unode = runtime.block_on(f).unwrap();

        // Make sure hash is the same if nothing was changed
        let f = derive_unode_manifest(
            ctx.clone(),
            repo.clone(),
            bcs_id,
            vec![p1_root_unode_id, p2_root_unode_id],
            get_file_changes(&bcs),
        );
        let same_root_unode = runtime.block_on(f).unwrap();
        assert_eq!(root_unode, same_root_unode);

        // Now change parent order, make sure hashes are different
        let f = derive_unode_manifest(
            ctx.clone(),
            repo.clone(),
            bcs_id,
            vec![p2_root_unode_id, p1_root_unode_id],
            get_file_changes(&bcs),
        );
        let reverse_root_unode = runtime.block_on(f).unwrap();

        assert_ne!(root_unode, reverse_root_unode);
    }

    fn create_changeset_and_derive_unode(
        ctx: CoreContext,
        repo: BlobRepo,
        mut runtime: &mut Runtime,
        file_changes: BTreeMap<&str, Option<(&str, FileType)>>,
    ) -> ManifestUnodeId {
        let file_changes = store_files(ctx.clone(), &mut runtime, file_changes, repo.clone());
        let bcs = create_bonsai_changeset(ctx.fb, repo.clone(), &mut runtime, file_changes);

        let bcs_id = bcs.get_changeset_id();
        let f = derive_unode_manifest(
            ctx.clone(),
            repo.clone(),
            bcs_id,
            vec![],
            get_file_changes(&bcs),
        );
        runtime.block_on(f).unwrap()
    }

    fn build_diamond_graph(
        ctx: CoreContext,
        mut runtime: &mut Runtime,
        repo: BlobRepo,
        changes_first: BTreeMap<&str, Option<(&str, FileType)>>,
        changes_merge_p1: BTreeMap<&str, Option<(&str, FileType)>>,
        changes_merge_p2: BTreeMap<&str, Option<(&str, FileType)>>,
        changes_merge: BTreeMap<&str, Option<(&str, FileType)>>,
    ) -> Result<ManifestUnodeId> {
        let file_changes = store_files(ctx.clone(), &mut runtime, changes_first, repo.clone());

        let bcs = create_bonsai_changeset(ctx.fb, repo.clone(), &mut runtime, file_changes);
        let first_bcs_id = bcs.get_changeset_id();

        let f = derive_unode_manifest(
            ctx.clone(),
            repo.clone(),
            first_bcs_id,
            vec![],
            get_file_changes(&bcs),
        );
        let first_unode_id = runtime.block_on(f).unwrap();

        let (merge_p1, merge_p1_unode_id) = {
            let file_changes =
                store_files(ctx.clone(), &mut runtime, changes_merge_p1, repo.clone());
            let merge_p1 = create_bonsai_changeset_with_params(
                ctx.fb,
                repo.clone(),
                &mut runtime,
                file_changes.clone(),
                "merge_p1",
                vec![first_bcs_id.clone()],
            );
            let merge_p1_id = merge_p1.get_changeset_id();
            let f = derive_unode_manifest(
                ctx.clone(),
                repo.clone(),
                merge_p1_id,
                vec![first_unode_id.clone()],
                get_file_changes(&merge_p1),
            );
            let merge_p1_unode_id = runtime.block_on(f).unwrap();
            (merge_p1, merge_p1_unode_id)
        };

        let (merge_p2, merge_p2_unode_id) = {
            let file_changes =
                store_files(ctx.clone(), &mut runtime, changes_merge_p2, repo.clone());

            let merge_p2 = create_bonsai_changeset_with_params(
                ctx.fb,
                repo.clone(),
                &mut runtime,
                file_changes,
                "merge_p2",
                vec![first_bcs_id.clone()],
            );
            let merge_p2_id = merge_p2.get_changeset_id();
            let f = derive_unode_manifest(
                ctx.clone(),
                repo.clone(),
                merge_p2_id,
                vec![first_unode_id.clone()],
                get_file_changes(&merge_p2),
            );
            let merge_p2_unode_id = runtime.block_on(f).unwrap();
            (merge_p2, merge_p2_unode_id)
        };

        let file_changes = store_files(ctx.clone(), &mut runtime, changes_merge, repo.clone());
        let merge = create_bonsai_changeset_with_params(
            ctx.fb,
            repo.clone(),
            &mut runtime,
            file_changes,
            "merge",
            vec![merge_p1.get_changeset_id(), merge_p2.get_changeset_id()],
        );
        let merge_id = merge.get_changeset_id();
        let f = derive_unode_manifest(
            ctx.clone(),
            repo.clone(),
            merge_id,
            vec![merge_p1_unode_id, merge_p2_unode_id],
            get_file_changes(&merge),
        );
        runtime.block_on(f)
    }

    fn create_bonsai_changeset(
        fb: FacebookInit,
        repo: BlobRepo,
        runtime: &mut Runtime,
        file_changes: BTreeMap<MPath, Option<FileChange>>,
    ) -> BonsaiChangeset {
        create_bonsai_changeset_with_params(fb, repo, runtime, file_changes, "message", vec![])
    }

    fn create_bonsai_changeset_with_params(
        fb: FacebookInit,
        repo: BlobRepo,
        runtime: &mut Runtime,
        file_changes: BTreeMap<MPath, Option<FileChange>>,
        message: &str,
        parents: Vec<ChangesetId>,
    ) -> BonsaiChangeset {
        let bcs = BonsaiChangesetMut {
            parents,
            author: "author".to_string(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: message.to_string(),
            extra: btreemap! {},
            file_changes,
        }
        .freeze()
        .unwrap();

        runtime
            .block_on(save_bonsai_changesets(
                vec![bcs.clone()],
                CoreContext::test_mock(fb),
                repo.clone(),
            ))
            .unwrap();
        bcs
    }

    fn store_files(
        ctx: CoreContext,
        runtime: &mut Runtime,
        files: BTreeMap<&str, Option<(&str, FileType)>>,
        repo: BlobRepo,
    ) -> BTreeMap<MPath, Option<FileChange>> {
        let mut res = btreemap! {};

        for (path, content) in files {
            let path = MPath::new(path).unwrap();
            match content {
                Some((content, file_type)) => {
                    let size = content.len();
                    let content =
                        FileContents::Bytes(Bytes::copy_from_slice(content.as_bytes())).into_blob();
                    let content_id = runtime
                        .block_on(content.store(ctx.clone(), repo.blobstore()))
                        .unwrap();

                    let file_change = FileChange::new(content_id, file_type, size as u64, None);
                    res.insert(path, Some(file_change));
                }
                None => {
                    res.insert(path, None);
                }
            }
        }
        res
    }

    trait UnodeHistory {
        fn get_parents(
            &self,
            ctx: CoreContext,
            repo: BlobRepo,
        ) -> BoxFuture<Vec<UnodeEntry>, Error>;

        fn get_linknode(&self, ctx: CoreContext, repo: BlobRepo) -> BoxFuture<ChangesetId, Error>;
    }

    impl UnodeHistory for UnodeEntry {
        fn get_parents(
            &self,
            ctx: CoreContext,
            repo: BlobRepo,
        ) -> BoxFuture<Vec<UnodeEntry>, Error> {
            match self {
                UnodeEntry::File(file_unode_id) => file_unode_id
                    .load(ctx, repo.blobstore())
                    .from_err()
                    .map(|unode_mf| {
                        unode_mf
                            .parents()
                            .into_iter()
                            .cloned()
                            .map(UnodeEntry::File)
                            .collect()
                    })
                    .boxify(),
                UnodeEntry::Directory(mf_unode_id) => mf_unode_id
                    .load(ctx, repo.blobstore())
                    .from_err()
                    .map(|unode_mf| {
                        unode_mf
                            .parents()
                            .into_iter()
                            .cloned()
                            .map(UnodeEntry::Directory)
                            .collect()
                    })
                    .boxify(),
            }
        }

        fn get_linknode(&self, ctx: CoreContext, repo: BlobRepo) -> BoxFuture<ChangesetId, Error> {
            match self {
                UnodeEntry::File(file_unode_id) => file_unode_id
                    .clone()
                    .load(ctx, repo.blobstore())
                    .from_err()
                    .map(|unode_file| unode_file.linknode().clone())
                    .boxify(),
                UnodeEntry::Directory(mf_unode_id) => mf_unode_id
                    .load(ctx, repo.blobstore())
                    .from_err()
                    .map(|unode_mf| unode_mf.linknode().clone())
                    .boxify(),
            }
        }
    }

    fn find_unode_history(
        fb: FacebookInit,
        runtime: &mut Runtime,
        repo: BlobRepo,
        start: UnodeEntry,
    ) -> Vec<ChangesetId> {
        let ctx = CoreContext::test_mock(fb);
        let mut q = VecDeque::new();
        q.push_back(start.clone());

        let mut visited = HashSet::new();
        visited.insert(start);
        let mut history = vec![];
        loop {
            let unode_entry = q.pop_front();
            let unode_entry = match unode_entry {
                Some(unode_entry) => unode_entry,
                None => {
                    break;
                }
            };
            let linknode = runtime
                .block_on(unode_entry.get_linknode(ctx.clone(), repo.clone()))
                .unwrap();
            history.push(linknode);
            let parents = runtime
                .block_on(unode_entry.get_parents(ctx.clone(), repo.clone()))
                .unwrap();
            q.extend(parents.into_iter().filter(|x| visited.insert(x.clone())));
        }

        history
    }

    fn find_filenode_history(
        fb: FacebookInit,
        runtime: &mut Runtime,
        repo: BlobRepo,
        start: HgFileNodeId,
    ) -> Vec<ChangesetId> {
        let ctx = CoreContext::test_mock(fb);

        let mut q = VecDeque::new();
        q.push_back(start);

        let mut visited = HashSet::new();
        visited.insert(start);
        let mut history = vec![];
        loop {
            let filenode_id = q.pop_front();
            let filenode_id = match filenode_id {
                Some(filenode_id) => filenode_id,
                None => {
                    break;
                }
            };

            let hg_linknode_fut = repo.get_linknode(ctx.clone(), &RepoPath::RootPath, filenode_id);
            let hg_linknode = runtime.block_on(hg_linknode_fut).unwrap();
            let hg_linknode = hg_linknode.do_not_handle_disabled_filenodes().unwrap();
            let linknode = runtime
                .block_on(repo.get_bonsai_from_hg(ctx.clone(), hg_linknode))
                .unwrap()
                .unwrap();
            history.push(linknode);

            let mf_fut = BlobManifest::load(
                ctx.clone(),
                repo.get_blobstore().boxed(),
                HgManifestId::new(filenode_id.into_nodehash()),
            );

            let mf = runtime.block_on(mf_fut).unwrap().unwrap();

            q.extend(
                mf.p1()
                    .into_iter()
                    .map(HgFileNodeId::new)
                    .filter(|x| visited.insert(*x)),
            );
            q.extend(
                mf.p2()
                    .into_iter()
                    .map(HgFileNodeId::new)
                    .filter(|x| visited.insert(*x)),
            );
        }

        history
    }

    fn fetch_root_filenode_id(
        fb: FacebookInit,
        runtime: &mut Runtime,
        repo: BlobRepo,
        bcs_id: ChangesetId,
    ) -> HgFileNodeId {
        let ctx = CoreContext::test_mock(fb);
        let hg_cs_id = runtime
            .block_on(repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id))
            .unwrap();
        let hg_cs = runtime
            .block_on(hg_cs_id.load(ctx.clone(), repo.blobstore()))
            .unwrap();

        HgFileNodeId::new(hg_cs.manifestid().into_nodehash())
    }
}
