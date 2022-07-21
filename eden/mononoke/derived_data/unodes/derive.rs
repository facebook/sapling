/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::format_err;
use anyhow::Error;
use blobstore::Blobstore;
use blobstore::Loadable;
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use futures::channel::mpsc;
use futures::future::try_join_all;
use futures::future::BoxFuture;
use futures::future::FutureExt as NewFutureExt;
use futures::future::TryFutureExt;
use futures::future::{self as new_future};
use manifest::derive_manifest_with_io_sender;
use manifest::derive_manifests_for_simple_stack_of_commits;
use manifest::Entry;
use manifest::LeafInfo;
use manifest::ManifestChanges;
use manifest::TreeInfo;
use metaconfig_types::UnodeVersion;
use mononoke_types::unode::FileUnode;
use mononoke_types::unode::ManifestUnode;
use mononoke_types::unode::UnodeEntry;
use mononoke_types::BlobstoreKey;
use mononoke_types::BlobstoreValue;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileType;
use mononoke_types::FileUnodeId;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use mononoke_types::MPathHash;
use mononoke_types::ManifestUnodeId;
use sorted_vector_map::SortedVectorMap;
use std::collections::BTreeMap;
use std::collections::HashMap;

use crate::ErrorKind;

pub(crate) async fn derive_unode_manifest_stack(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    file_changes: Vec<(ChangesetId, BTreeMap<MPath, Option<(ContentId, FileType)>>)>,
    parent: Option<ManifestUnodeId>,
    unode_version: UnodeVersion,
) -> Result<HashMap<ChangesetId, ManifestUnodeId>, Error> {
    let blobstore = derivation_ctx.blobstore();

    let manifest_changes = file_changes
        .into_iter()
        .map(|(cs_id, file_changes)| ManifestChanges {
            cs_id,
            changes: file_changes.into_iter().collect(),
        })
        .collect::<Vec<_>>();

    let res = derive_manifests_for_simple_stack_of_commits(
        ctx.clone(),
        blobstore.clone(),
        parent,
        manifest_changes,
        {
            cloned!(ctx, blobstore);
            move |tree_info, cs_id| {
                create_unode_manifest(
                    ctx.clone(),
                    cs_id,
                    blobstore.clone(),
                    None,
                    tree_info,
                    unode_version,
                )
            }
        },
        {
            cloned!(ctx, blobstore);
            move |leaf_info, cs_id| {
                create_unode_file(
                    ctx.clone(),
                    cs_id,
                    blobstore.clone(),
                    None,
                    leaf_info,
                    unode_version,
                )
            }
        },
    )
    .await?;

    Ok(res.into_iter().collect())
}

/// Derives unode manifests for bonsai changeset `cs_id` given parent unode manifests.
/// Note that `derive_manifest()` does a lot of the heavy lifting for us, and this crate has to
/// provide only functions to create a single unode file or single unode tree (
/// `create_unode_manifest` and `create_unode_file`).
pub(crate) async fn derive_unode_manifest(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    cs_id: ChangesetId,
    parents: Vec<ManifestUnodeId>,
    changes: Vec<(MPath, Option<(ContentId, FileType)>)>,
    unode_version: UnodeVersion,
) -> Result<ManifestUnodeId, Error> {
    let parents: Vec<_> = parents.into_iter().collect();
    let blobstore = derivation_ctx.blobstore();

    let maybe_tree_id = derive_manifest_with_io_sender(
        ctx.clone(),
        blobstore.clone(),
        parents.clone(),
        changes,
        {
            cloned!(ctx, blobstore);
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
            cloned!(ctx, blobstore);
            move |leaf_info, sender| {
                create_unode_file(
                    ctx.clone(),
                    cs_id,
                    blobstore.clone(),
                    Some(sender),
                    leaf_info,
                    unode_version,
                )
            }
        },
    )
    .await?;

    match maybe_tree_id {
        Some(tree_id) => Ok(tree_id),
        None => {
            // All files have been deleted, generate empty **root** manifest
            let tree_info = TreeInfo {
                path: None,
                parents,
                subentries: Default::default(),
            };
            let ((), tree_id) = create_unode_manifest(
                ctx.clone(),
                cs_id,
                blobstore.clone(),
                None,
                tree_info,
                unode_version,
            )
            .await?;
            Ok(tree_id)
        }
    }
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
async fn create_unode_manifest(
    ctx: CoreContext,
    linknode: ChangesetId,
    blobstore: Arc<dyn Blobstore>,
    sender: Option<mpsc::UnboundedSender<BoxFuture<'static, Result<(), Error>>>>,
    tree_info: TreeInfo<ManifestUnodeId, FileUnodeId, ()>,
    unode_version: UnodeVersion,
) -> Result<((), ManifestUnodeId), Error> {
    let mut subentries = SortedVectorMap::new();
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
    if can_reuse(unode_version) && tree_info.parents.len() > 1 {
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
    let f = async move { blobstore.put(&ctx, key, blob.into()).await };

    match sender {
        Some(sender) => sender
            .unbounded_send(f.boxed())
            .map_err(|err| format_err!("failed to send manifest future {}", err))?,
        None => f.await?,
    };
    Ok(((), mf_unode_id))
}

async fn create_unode_file(
    ctx: CoreContext,
    linknode: ChangesetId,
    blobstore: Arc<dyn Blobstore>,
    sender: Option<mpsc::UnboundedSender<BoxFuture<'static, Result<(), Error>>>>,
    leaf_info: LeafInfo<FileUnodeId, (ContentId, FileType)>,
    unode_version: UnodeVersion,
) -> Result<((), FileUnodeId), Error> {
    borrowed!(ctx, blobstore);

    async fn save_unode(
        ctx: &CoreContext,
        blobstore: &Arc<dyn Blobstore>,
        sender: Option<mpsc::UnboundedSender<BoxFuture<'static, Result<(), Error>>>>,
        parents: Vec<FileUnodeId>,
        content_id: ContentId,
        file_type: FileType,
        path_hash: MPathHash,
        linknode: ChangesetId,
        unode_version: UnodeVersion,
    ) -> Result<FileUnodeId, Error> {
        if can_reuse(unode_version) && parents.len() > 1 {
            if let Some(parent) =
                reuse_file_parent(ctx, blobstore, &parents, &content_id, file_type).await?
            {
                return Ok(parent);
            }
        };
        let file_unode = FileUnode::new(parents, content_id, file_type, path_hash, linknode);
        let file_unode_id = file_unode.get_unode_id();

        let f = {
            cloned!(ctx, blobstore);
            async move {
                blobstore
                    .put(
                        &ctx,
                        file_unode_id.blobstore_key(),
                        file_unode.into_blob().into(),
                    )
                    .await
            }
            .boxed()
        };

        match sender {
            Some(sender) => {
                sender
                    .unbounded_send(f)
                    .map_err(|err| format_err!("failed to send manifest future {}", err))?;
            }
            None => f.await?,
        };
        Ok(file_unode_id)
    }

    let LeafInfo {
        leaf,
        path,
        parents,
    } = leaf_info;

    let leaf_id = if let Some((content_id, file_type)) = leaf {
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
        .await?
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
            return Err(ErrorKind::InvalidBonsai(
                "no change is provided, but file unode has only one parent".to_string(),
            )
            .into());
        }
        let parent_unodes = try_join_all(
            parents
                .clone()
                .into_iter()
                .map(|id| async move { id.load(ctx, blobstore).await }),
        )
        .await?;

        match return_if_unique_filenode(&parent_unodes) {
            Some((content_id, file_type)) => {
                save_unode(
                    ctx,
                    blobstore,
                    sender,
                    parents,
                    content_id.clone(),
                    *file_type,
                    path.get_path_hash(),
                    linknode,
                    unode_version,
                )
                .await?
            }
            _ => {
                return Err(ErrorKind::InvalidBonsai(
                    "no change is provided, but content is different".to_string(),
                )
                .into());
            }
        }
    };

    Ok(((), leaf_id))
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

fn can_reuse(unode_version: UnodeVersion) -> bool {
    unode_version == UnodeVersion::V2 || tunables::tunables().get_force_unode_v2()
}

async fn reuse_manifest_parent(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    parents: &[ManifestUnodeId],
    subentries: &SortedVectorMap<MPathElement, UnodeEntry>,
) -> Result<Option<ManifestUnodeId>, Error> {
    let parents = new_future::try_join_all(
        parents
            .iter()
            .map(|id| id.load(ctx, blobstore).map_err(Error::from)),
    )
    .await?;

    if let Some(mf_unode) = parents.iter().find(|p| p.subentries() == subentries) {
        Ok(Some(mf_unode.get_unode_id()))
    } else {
        Ok(None)
    }
}

async fn reuse_file_parent(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    parents: &[FileUnodeId],
    content_id: &ContentId,
    file_type: FileType,
) -> Result<Option<FileUnodeId>, Error> {
    let parents = new_future::try_join_all(
        parents
            .iter()
            .map(|id| id.load(ctx, blobstore).map_err(Error::from)),
    )
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
fn return_if_unique_filenode(unodes: &[FileUnode]) -> Option<(&ContentId, &FileType)> {
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
    use crate::mapping::get_file_changes;
    use crate::mapping::RootUnodeManifestId;
    use anyhow::Result;
    use async_trait::async_trait;
    use blobrepo::save_bonsai_changesets;
    use blobrepo::BlobRepo;
    use blobrepo_hg::BlobRepoHg;
    use blobstore::Storable;
    use bytes::Bytes;
    use derived_data::BonsaiDerived;
    use derived_data_filenodes::FilenodesOnlyPublic;
    use derived_data_test_utils::bonsai_changeset_from_hg;
    use derived_data_test_utils::iterate_all_manifest_entries;
    use fbinit::FacebookInit;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use futures::TryStreamExt;
    use manifest::ManifestOps;
    use maplit::btreemap;
    use mercurial_derived_data::DeriveHgChangeset;
    use mercurial_types::blobs::HgBlobManifest;
    use mercurial_types::HgFileNodeId;
    use mercurial_types::HgManifestId;
    use mononoke_types::BlobstoreValue;
    use mononoke_types::BonsaiChangeset;
    use mononoke_types::BonsaiChangesetMut;
    use mononoke_types::DateTime;
    use mononoke_types::FileChange;
    use mononoke_types::FileContents;
    use mononoke_types::RepoPath;
    use repo_derived_data::RepoDerivedDataRef;
    use std::collections::BTreeMap;
    use std::collections::HashSet;
    use std::collections::VecDeque;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::resolve_cs_id;
    use tests_utils::CreateCommitContext;

    #[fbinit::test]
    async fn linear_test(fb: FacebookInit) -> Result<(), Error> {
        let repo = Linear::getrepo(fb).await;
        let derivation_ctx = repo.repo_derived_data().manager().derivation_context(None);
        let ctx = CoreContext::test_mock(fb);

        // Derive filenodes because they are going to be used in this test
        let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
        FilenodesOnlyPublic::derive(&ctx, &repo, master_cs_id).await?;

        let parent_unode_id = {
            let parent_hg_cs = "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536";
            let (bcs_id, bcs) = bonsai_changeset_from_hg(&ctx, &repo, parent_hg_cs).await?;
            let unode_id = derive_unode_manifest(
                &ctx,
                &derivation_ctx,
                bcs_id,
                vec![],
                get_file_changes(&bcs),
                UnodeVersion::V2,
            )
            .await?;

            // Make sure it's saved in the blobstore
            unode_id.load(&ctx, repo.blobstore()).await?;
            let all_unodes: Vec<_> =
                iterate_all_manifest_entries(&ctx, &repo, Entry::Tree(unode_id))
                    .try_collect()
                    .await?;
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
            let (bcs_id, bcs) = bonsai_changeset_from_hg(&ctx, &repo, child_hg_cs).await?;

            let unode_id = derive_unode_manifest(
                &ctx,
                &derivation_ctx,
                bcs_id,
                vec![parent_unode_id.clone()],
                get_file_changes(&bcs),
                UnodeVersion::V2,
            )
            .await?;

            // Make sure it's saved in the blobstore
            let root_unode = unode_id.load(&ctx, repo.blobstore()).await?;
            assert_eq!(root_unode.parents(), &vec![parent_unode_id]);

            let root_filenode_id = fetch_root_filenode_id(fb, repo.clone(), bcs_id).await?;
            assert_eq!(
                find_unode_history(fb, repo.clone(), UnodeEntry::Directory(unode_id)).await?,
                find_filenode_history(fb, repo.clone(), root_filenode_id).await?,
            );

            let all_unodes: Vec<_> =
                iterate_all_manifest_entries(&ctx, &repo, Entry::Tree(unode_id))
                    .try_collect()
                    .await?;
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
    async fn test_same_content_different_paths(fb: FacebookInit) -> Result<(), Error> {
        let repo = Linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        async fn check_unode_uniqeness(
            ctx: CoreContext,
            repo: BlobRepo,
            file_changes: BTreeMap<MPath, FileChange>,
        ) -> Result<(), Error> {
            let derivation_ctx = repo.repo_derived_data().manager().derivation_context(None);
            let bcs = create_bonsai_changeset(ctx.fb, repo.clone(), file_changes).await?;
            let bcs_id = bcs.get_changeset_id();

            let unode_id = derive_unode_manifest(
                &ctx,
                &derivation_ctx,
                bcs_id,
                vec![],
                get_file_changes(&bcs),
                UnodeVersion::V2,
            )
            .await?;
            let unode_mf = unode_id.load(&ctx, repo.blobstore()).await?;

            // Unodes should be unique even if content is the same. Check it
            let vals: Vec<_> = unode_mf.list().collect();
            assert_eq!(vals.len(), 2);
            assert_ne!(vals.get(0), vals.get(1));
            Ok(())
        }

        let file_changes = store_files(
            ctx.clone(),
            btreemap! {"file1" => Some(("content", FileType::Regular)), "file2" => Some(("content", FileType::Regular))},
            repo.clone(),
        ).await?;
        check_unode_uniqeness(ctx.clone(), repo.clone(), file_changes).await?;

        let file_changes = store_files(
            ctx.clone(),
            btreemap! {"dir1/file" => Some(("content", FileType::Regular)), "dir2/file" => Some(("content", FileType::Regular))},
            repo.clone(),
        ).await?;
        check_unode_uniqeness(ctx.clone(), repo.clone(), file_changes).await?;
        Ok(())
    }

    #[fbinit::test]
    async fn test_same_content_no_change(fb: FacebookInit) -> Result<(), Error> {
        let repo = Linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        build_diamond_graph(
            ctx.clone(),
            repo.clone(),
            btreemap! {"A" => Some(("A", FileType::Regular))},
            btreemap! {"A" => Some(("B", FileType::Regular))},
            btreemap! {"A" => Some(("B", FileType::Regular))},
            btreemap! {},
        )
        .await?;

        // Content is different - fail!
        assert!(
            build_diamond_graph(
                ctx.clone(),
                repo.clone(),
                btreemap! {"A" => Some(("A", FileType::Regular))},
                btreemap! {"A" => Some(("B", FileType::Regular))},
                btreemap! {"A" => Some(("C", FileType::Regular))},
                btreemap! {},
            )
            .await
            .is_err()
        );

        // Type is different - fail!
        assert!(
            build_diamond_graph(
                ctx,
                repo,
                btreemap! {"A" => Some(("A", FileType::Regular))},
                btreemap! {"A" => Some(("B", FileType::Regular))},
                btreemap! {"A" => Some(("B", FileType::Executable))},
                btreemap! {},
            )
            .await
            .is_err()
        );

        Ok(())
    }

    async fn diamond_merge_unodes_v2(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut factory = TestRepoFactory::new(fb)?;
        let repo: BlobRepo = factory.build()?;
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
                let p1_root_unode_mf_id = RootUnodeManifestId::derive(&ctx, &repo, p1).await?;

                let mut p1_unodes: Vec<_> = p1_root_unode_mf_id
                    .manifest_unode_id()
                    .find_entries(
                        ctx.clone(),
                        repo.get_blobstore(),
                        vec![Some(MPath::new(&merged_files)?), Some(MPath::new("dir")?)],
                        // Some(MPath::new(&merged_files)?),
                    )
                    .try_collect()
                    .await?;
                p1_unodes.sort_by_key(|(path, _)| path.clone());

                let merge_root_unode_mf_id =
                    RootUnodeManifestId::derive(&ctx, &repo, merge).await?;

                let mut merge_unodes: Vec<_> = merge_root_unode_mf_id
                    .manifest_unode_id()
                    .find_entries(
                        ctx.clone(),
                        repo.get_blobstore(),
                        vec![Some(MPath::new(&merged_files)?), Some(MPath::new("dir")?)],
                    )
                    .try_collect()
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
        let repo: BlobRepo = factory
            .with_config_override(|config| {
                config
                    .derived_data_config
                    .get_active_config()
                    .expect("No enabled derived data types config")
                    .unode_version = UnodeVersion::V1;
            })
            .build()?;
        let (p1_unodes, merge_unodes) = find_unodes(ctx.clone(), repo.clone()).await?;
        assert_ne!(p1_unodes, merge_unodes);

        for ((_, p1), (_, merge)) in p1_unodes.iter().zip(merge_unodes.iter()) {
            let merge_unode = merge.load(&ctx, repo.blobstore()).await?;

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
    async fn test_diamond_merge_unodes_v2(fb: FacebookInit) -> Result<(), Error> {
        diamond_merge_unodes_v2(fb).await
    }

    #[fbinit::test]
    async fn test_parent_order(fb: FacebookInit) -> Result<(), Error> {
        let repo: BlobRepo = test_repo_factory::build_empty(fb).unwrap();
        let derivation_ctx = repo.repo_derived_data().manager().derivation_context(None);
        let ctx = CoreContext::test_mock(fb);

        let p1_root_unode_id = create_changeset_and_derive_unode(
            ctx.clone(),
            repo.clone(),
            btreemap! {"A" => Some(("A", FileType::Regular))},
        )
        .await?;

        let p2_root_unode_id = create_changeset_and_derive_unode(
            ctx.clone(),
            repo.clone(),
            btreemap! {"A" => Some(("B", FileType::Regular))},
        )
        .await?;

        let file_changes = store_files(
            ctx.clone(),
            btreemap! { "A" => Some(("C", FileType::Regular)) },
            repo.clone(),
        )
        .await?;
        let bcs = create_bonsai_changeset(fb, repo.clone(), file_changes).await?;
        let bcs_id = bcs.get_changeset_id();

        let root_unode = derive_unode_manifest(
            &ctx,
            &derivation_ctx,
            bcs_id,
            vec![p1_root_unode_id, p2_root_unode_id],
            get_file_changes(&bcs),
            UnodeVersion::V2,
        )
        .await?;

        // Make sure hash is the same if nothing was changed
        let same_root_unode = derive_unode_manifest(
            &ctx,
            &derivation_ctx,
            bcs_id,
            vec![p1_root_unode_id, p2_root_unode_id],
            get_file_changes(&bcs),
            UnodeVersion::V2,
        )
        .await?;
        assert_eq!(root_unode, same_root_unode);

        // Now change parent order, make sure hashes are different
        let reverse_root_unode = derive_unode_manifest(
            &ctx,
            &derivation_ctx,
            bcs_id,
            vec![p2_root_unode_id, p1_root_unode_id],
            get_file_changes(&bcs),
            UnodeVersion::V2,
        )
        .await?;

        assert_ne!(root_unode, reverse_root_unode);

        Ok(())
    }

    async fn create_changeset_and_derive_unode(
        ctx: CoreContext,
        repo: BlobRepo,
        file_changes: BTreeMap<&str, Option<(&str, FileType)>>,
    ) -> Result<ManifestUnodeId, Error> {
        let file_changes = store_files(ctx.clone(), file_changes, repo.clone()).await?;
        let bcs = create_bonsai_changeset(ctx.fb, repo.clone(), file_changes).await?;

        let bcs_id = bcs.get_changeset_id();
        let derivation_ctx = repo.repo_derived_data().manager().derivation_context(None);

        derive_unode_manifest(
            &ctx,
            &derivation_ctx,
            bcs_id,
            vec![],
            get_file_changes(&bcs),
            UnodeVersion::V2,
        )
        .await
    }

    async fn build_diamond_graph(
        ctx: CoreContext,
        repo: BlobRepo,
        changes_first: BTreeMap<&str, Option<(&str, FileType)>>,
        changes_merge_p1: BTreeMap<&str, Option<(&str, FileType)>>,
        changes_merge_p2: BTreeMap<&str, Option<(&str, FileType)>>,
        changes_merge: BTreeMap<&str, Option<(&str, FileType)>>,
    ) -> Result<ManifestUnodeId> {
        let derivation_ctx = repo.repo_derived_data().manager().derivation_context(None);
        let file_changes = store_files(ctx.clone(), changes_first, repo.clone()).await?;

        let bcs = create_bonsai_changeset(ctx.fb, repo.clone(), file_changes).await?;
        let first_bcs_id = bcs.get_changeset_id();

        let first_unode_id = derive_unode_manifest(
            &ctx,
            &derivation_ctx,
            first_bcs_id,
            vec![],
            get_file_changes(&bcs),
            UnodeVersion::V2,
        )
        .await?;

        let (merge_p1, merge_p1_unode_id) = {
            let file_changes = store_files(ctx.clone(), changes_merge_p1, repo.clone()).await?;
            let merge_p1 = create_bonsai_changeset_with_params(
                ctx.fb,
                repo.clone(),
                file_changes.clone(),
                "merge_p1",
                vec![first_bcs_id.clone()],
            )
            .await?;
            let merge_p1_id = merge_p1.get_changeset_id();
            let merge_p1_unode_id = derive_unode_manifest(
                &ctx,
                &derivation_ctx,
                merge_p1_id,
                vec![first_unode_id.clone()],
                get_file_changes(&merge_p1),
                UnodeVersion::V2,
            )
            .await?;
            (merge_p1, merge_p1_unode_id)
        };

        let (merge_p2, merge_p2_unode_id) = {
            let file_changes = store_files(ctx.clone(), changes_merge_p2, repo.clone()).await?;

            let merge_p2 = create_bonsai_changeset_with_params(
                ctx.fb,
                repo.clone(),
                file_changes,
                "merge_p2",
                vec![first_bcs_id.clone()],
            )
            .await?;
            let merge_p2_id = merge_p2.get_changeset_id();
            let merge_p2_unode_id = derive_unode_manifest(
                &ctx,
                &derivation_ctx,
                merge_p2_id,
                vec![first_unode_id.clone()],
                get_file_changes(&merge_p2),
                UnodeVersion::V2,
            )
            .await?;
            (merge_p2, merge_p2_unode_id)
        };

        let file_changes = store_files(ctx.clone(), changes_merge, repo.clone()).await?;
        let merge = create_bonsai_changeset_with_params(
            ctx.fb,
            repo.clone(),
            file_changes,
            "merge",
            vec![merge_p1.get_changeset_id(), merge_p2.get_changeset_id()],
        )
        .await?;
        let merge_id = merge.get_changeset_id();
        derive_unode_manifest(
            &ctx,
            &derivation_ctx,
            merge_id,
            vec![merge_p1_unode_id, merge_p2_unode_id],
            get_file_changes(&merge),
            UnodeVersion::V2,
        )
        .await
    }

    async fn create_bonsai_changeset(
        fb: FacebookInit,
        repo: BlobRepo,
        file_changes: BTreeMap<MPath, FileChange>,
    ) -> Result<BonsaiChangeset, Error> {
        create_bonsai_changeset_with_params(fb, repo, file_changes, "message", vec![]).await
    }

    async fn create_bonsai_changeset_with_params(
        fb: FacebookInit,
        repo: BlobRepo,
        file_changes: BTreeMap<MPath, FileChange>,
        message: &str,
        parents: Vec<ChangesetId>,
    ) -> Result<BonsaiChangeset, Error> {
        let bcs = BonsaiChangesetMut {
            parents,
            author: "author".to_string(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: message.to_string(),
            extra: Default::default(),
            file_changes: file_changes.into(),
            is_snapshot: false,
        }
        .freeze()
        .unwrap();

        save_bonsai_changesets(vec![bcs.clone()], CoreContext::test_mock(fb), &repo).await?;
        Ok(bcs)
    }

    async fn store_files(
        ctx: CoreContext,
        files: BTreeMap<&str, Option<(&str, FileType)>>,
        repo: BlobRepo,
    ) -> Result<BTreeMap<MPath, FileChange>, Error> {
        let mut res = btreemap! {};

        for (path, content) in files {
            let path = MPath::new(path).unwrap();
            match content {
                Some((content, file_type)) => {
                    let size = content.len();
                    let content =
                        FileContents::Bytes(Bytes::copy_from_slice(content.as_bytes())).into_blob();
                    let content_id = content.store(&ctx, repo.blobstore()).await?;
                    let file_change = FileChange::tracked(content_id, file_type, size as u64, None);
                    res.insert(path, file_change);
                }
                None => {
                    res.insert(path, FileChange::Deletion);
                }
            }
        }
        Ok(res)
    }

    #[async_trait]
    trait UnodeHistory {
        async fn get_parents<'a>(
            &'a self,
            ctx: &'a CoreContext,
            repo: &'a BlobRepo,
        ) -> Result<Vec<UnodeEntry>, Error>;

        async fn get_linknode<'a>(
            &'a self,
            ctx: &'a CoreContext,
            repo: &'a BlobRepo,
        ) -> Result<ChangesetId, Error>;
    }

    #[async_trait]
    impl UnodeHistory for UnodeEntry {
        async fn get_parents<'a>(
            &'a self,
            ctx: &'a CoreContext,
            repo: &'a BlobRepo,
        ) -> Result<Vec<UnodeEntry>, Error> {
            match self {
                UnodeEntry::File(file_unode_id) => {
                    let unode_mf = file_unode_id.load(ctx, repo.blobstore()).await?;
                    Ok(unode_mf
                        .parents()
                        .iter()
                        .cloned()
                        .map(UnodeEntry::File)
                        .collect())
                }
                UnodeEntry::Directory(mf_unode_id) => {
                    let unode_mf = mf_unode_id.load(ctx, repo.blobstore()).await?;
                    Ok(unode_mf
                        .parents()
                        .iter()
                        .cloned()
                        .map(UnodeEntry::Directory)
                        .collect())
                }
            }
        }

        async fn get_linknode<'a>(
            &'a self,
            ctx: &'a CoreContext,
            repo: &'a BlobRepo,
        ) -> Result<ChangesetId, Error> {
            match self {
                UnodeEntry::File(file_unode_id) => {
                    let unode_file = file_unode_id.load(ctx, repo.blobstore()).await?;
                    Ok(unode_file.linknode().clone())
                }
                UnodeEntry::Directory(mf_unode_id) => {
                    let unode_mf = mf_unode_id.load(ctx, repo.blobstore()).await?;
                    Ok(unode_mf.linknode().clone())
                }
            }
        }
    }

    async fn find_unode_history(
        fb: FacebookInit,
        repo: BlobRepo,
        start: UnodeEntry,
    ) -> Result<Vec<ChangesetId>, Error> {
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
            let linknode = unode_entry.get_linknode(&ctx, &repo).await?;
            history.push(linknode);
            let parents = unode_entry.get_parents(&ctx, &repo).await?;
            q.extend(parents.into_iter().filter(|x| visited.insert(x.clone())));
        }

        Ok(history)
    }

    async fn find_filenode_history(
        fb: FacebookInit,
        repo: BlobRepo,
        start: HgFileNodeId,
    ) -> Result<Vec<ChangesetId>, Error> {
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

            let hg_linknode = repo
                .get_filenode(ctx.clone(), &RepoPath::RootPath, filenode_id)
                .await?
                .map(|filenode| filenode.linknode)
                .do_not_handle_disabled_filenodes()?;
            let linknode = repo
                .bonsai_hg_mapping()
                .get_bonsai_from_hg(&ctx, hg_linknode)
                .await?
                .unwrap();
            history.push(linknode);

            let mf = HgBlobManifest::load(
                &ctx,
                repo.blobstore(),
                HgManifestId::new(filenode_id.into_nodehash()),
            )
            .await?
            .unwrap();

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

        Ok(history)
    }

    async fn fetch_root_filenode_id(
        fb: FacebookInit,
        repo: BlobRepo,
        bcs_id: ChangesetId,
    ) -> Result<HgFileNodeId, Error> {
        let ctx = CoreContext::test_mock(fb);
        let hg_cs_id = repo.derive_hg_changeset(&ctx, bcs_id).await?;
        let hg_cs = hg_cs_id.load(&ctx, repo.blobstore()).await?;
        Ok(HgFileNodeId::new(hg_cs.manifestid().into_nodehash()))
    }
}
