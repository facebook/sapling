// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use blobstore::Blobstore;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{Error, Fail};
use futures::{future, Future};
use futures_ext::{BoxFuture, FutureExt};
use manifest::{derive_manifest, Entry, LeafInfo, Manifest, TreeInfo};
use mononoke_types::unode::{FileUnode, ManifestUnode, UnodeEntry};
use mononoke_types::{
    BlobstoreValue, ChangesetId, ContentId, FileType, FileUnodeId, Loadable, MPathHash,
    ManifestUnodeId, MononokeId,
};
use mononoke_types::{MPath, MPathElement};
use repo_blobstore::RepoBlobstore;
use std::collections::BTreeMap;

pub mod derived_data_unodes;
#[cfg(test)]
mod test_utils;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "cannot fetch FileUnode: {}", _0)]
    FailFetchFileUnode(FileUnodeId),
    #[fail(display = "cannot fetch ManifestUnode: {}", _0)]
    FailFetchManifestUnode(ManifestUnodeId),
    #[fail(display = "Invalid bonsai changeset: {}", _0)]
    InvalidBonsai(String),
}

// Id type is added so that we can implement Loadable trait for ManifestUnodeId
// We can't do it now because ManifestUnodeId and Loadable are defined in different crates.
// TODO(stash): move Loadable trait implementation into mononoke_types?
#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct Id<T>(T);

impl Loadable for Id<ManifestUnodeId> {
    type Value = Id<ManifestUnode>;

    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Value, Error> {
        let unode_id = self.0;
        blobstore
            .get(ctx, unode_id.blobstore_key())
            .and_then(move |bytes| match bytes {
                None => Err(ErrorKind::FailFetchManifestUnode(unode_id).into()),
                Some(bytes) => ManifestUnode::from_bytes(bytes.as_bytes().as_ref()).map(Id),
            })
            .boxify()
    }
}

impl Loadable for Id<FileUnodeId> {
    type Value = Id<FileUnode>;

    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Value, Error> {
        let unode_id = self.0;
        blobstore
            .get(ctx, unode_id.blobstore_key())
            .and_then(move |bytes| match bytes {
                None => Err(ErrorKind::FailFetchFileUnode(unode_id).into()),
                Some(bytes) => FileUnode::from_bytes(bytes.as_bytes().as_ref()).map(Id),
            })
            .boxify()
    }
}

impl Manifest for Id<ManifestUnode> {
    type TreeId = Id<ManifestUnodeId>;
    type LeafId = FileUnodeId;

    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
        self.0.lookup(name).map(convert_unode)
    }

    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
        let v: Vec<_> = self
            .0
            .list()
            .map(|(basename, entry)| (basename.clone(), convert_unode(entry)))
            .collect();
        Box::new(v.into_iter())
    }
}

/// Derives unode manifests for bonsai changeset `cs_id` given parent unode manifests.
/// Note that `derive_manifest()` does a lot of the heavy lifting for us, and this crate has to
/// provide only functions to create a single unode file or single unode tree (
/// `create_unode_manifest` and `create_unode_file`).
pub fn derive_unode_manifest(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
    parents: impl IntoIterator<Item = ManifestUnodeId>,
    changes: impl IntoIterator<Item = (MPath, Option<(ContentId, FileType)>)>,
) -> impl Future<Item = ManifestUnodeId, Error = Error> {
    let parents: Vec<_> = parents.into_iter().map(Id).collect();

    derive_manifest(
        ctx.clone(),
        repo.get_blobstore(),
        parents.clone(),
        changes,
        {
            cloned!(ctx, cs_id, repo);
            move |tree_info| {
                create_unode_manifest(ctx.clone(), cs_id, repo.get_blobstore(), tree_info)
            }
        },
        {
            cloned!(ctx, cs_id, repo);
            move |leaf_info| create_unode_file(ctx.clone(), cs_id, repo.get_blobstore(), leaf_info)
        },
    )
    .and_then(move |maybe_tree_id| match maybe_tree_id {
        Some(tree_id) => future::ok(tree_id.0).left_future(),
        None => {
            // All files have been deleted, generate empty **root** manifest
            let tree_info = TreeInfo {
                path: None,
                parents,
                subentries: Default::default(),
            };
            create_unode_manifest(ctx, cs_id, repo.get_blobstore(), tree_info)
                .map(|id| id.0)
                .right_future()
        }
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
    tree_info: TreeInfo<Id<ManifestUnodeId>, FileUnodeId>,
) -> impl Future<Item = Id<ManifestUnodeId>, Error = Error> {
    let mut subentries = BTreeMap::new();
    for (basename, entry) in tree_info.subentries {
        match entry {
            Entry::Tree(mf_unode) => {
                subentries.insert(basename, UnodeEntry::Directory(mf_unode.0));
            }
            Entry::Leaf(file_unode) => {
                subentries.insert(basename, UnodeEntry::File(file_unode));
            }
        }
    }
    let parents: Vec<_> = tree_info.parents.into_iter().map(|id| id.0).collect();
    let mf_unode = ManifestUnode::new(parents, subentries, linknode);
    let mf_unode_id = mf_unode.get_unode_id();
    let key = mf_unode_id.blobstore_key();
    let blob = mf_unode.into_blob();
    blobstore
        .put(ctx, key, blob.into())
        .map(move |()| Id(mf_unode_id))
}

fn create_unode_file(
    ctx: CoreContext,
    linknode: ChangesetId,
    blobstore: RepoBlobstore,
    leaf_info: LeafInfo<FileUnodeId, (ContentId, FileType)>,
) -> BoxFuture<FileUnodeId, Error> {
    fn save_unode(
        ctx: CoreContext,
        blobstore: RepoBlobstore,
        parents: Vec<FileUnodeId>,
        content_id: ContentId,
        file_type: FileType,
        path_hash: MPathHash,
        linknode: ChangesetId,
    ) -> BoxFuture<FileUnodeId, Error> {
        let file_unode = FileUnode::new(parents, content_id, file_type, path_hash, linknode);
        let file_unode_id = file_unode.get_unode_id();
        blobstore
            .put(
                ctx,
                file_unode_id.blobstore_key(),
                file_unode.into_blob().into(),
            )
            .map(move |()| file_unode_id)
            .boxify()
    }

    let LeafInfo {
        leaf,
        path,
        parents,
    } = leaf_info;

    if let Some((content_id, file_type)) = leaf {
        save_unode(
            ctx.clone(),
            blobstore,
            parents,
            content_id,
            file_type,
            path.get_path_hash(),
            linknode,
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
            move |id| Id(id).load(ctx.clone(), &blobstore)
        }))
        .and_then(
            move |parent_unodes| match return_if_unique_filenode(&parent_unodes) {
                Some((content_id, file_type)) => save_unode(
                    ctx,
                    blobstore,
                    parents,
                    content_id.clone(),
                    *file_type,
                    path.get_path_hash(),
                    linknode,
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
}

// If all elements in `unodes` are the same than this element is returned, otherwise None is returned
fn return_if_unique_filenode(unodes: &Vec<Id<FileUnode>>) -> Option<(&ContentId, &FileType)> {
    let mut iter = unodes
        .iter()
        .map(|elem| (elem.0.content_id(), elem.0.file_type()));
    let first_elem = iter.next()?;
    if iter.all(|next_elem| next_elem == first_elem) {
        Some(first_elem)
    } else {
        None
    }
}

fn convert_unode(unode_entry: &UnodeEntry) -> Entry<Id<ManifestUnodeId>, FileUnodeId> {
    match unode_entry {
        UnodeEntry::File(file_unode_id) => Entry::Leaf(file_unode_id.clone()),
        UnodeEntry::Directory(mf_unode_id) => Entry::Tree(Id(mf_unode_id.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{get_bonsai_changeset, iterate_all_entries};
    use blobrepo::{save_bonsai_changesets, BlobManifest};
    use blobrepo_factory::new_memblob_empty;
    use bytes::Bytes;
    use derived_data_unodes::get_file_changes;
    use failure_ext::Result;
    use fixtures::linear;
    use futures::Stream;
    use manifest::Manifest;
    use maplit::btreemap;
    use mercurial_types::{Changeset, HgFileNodeId, HgManifestId};
    use mononoke_types::{
        BonsaiChangeset, BonsaiChangesetMut, DateTime, FileChange, FileContents, RepoPath,
    };
    use std::collections::{HashSet, VecDeque};
    use tokio::runtime::Runtime;

    #[test]
    fn linear_test() {
        let repo = linear::getrepo();
        let mut runtime = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock();
        let parent_unode_id = {
            let parent_hg_cs = "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536";
            let (bcs_id, bcs) =
                get_bonsai_changeset(ctx.clone(), repo.clone(), &mut runtime, parent_hg_cs);

            let f = derive_unode_manifest(
                ctx.clone(),
                repo.clone(),
                bcs_id,
                vec![].into_iter(),
                get_file_changes(&bcs),
            );

            let unode_id = runtime.block_on(f).unwrap();
            // Make sure it's saved in the blobstore
            runtime
                .block_on(Id(unode_id).load(ctx.clone(), &repo.get_blobstore()))
                .unwrap();
            let all_unodes = runtime
                .block_on(
                    iterate_all_entries(ctx.clone(), repo.clone(), Entry::Tree(Id(unode_id)))
                        .collect(),
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
                vec![parent_unode_id.clone()].into_iter(),
                get_file_changes(&bcs),
            );

            let unode_id = runtime.block_on(f).unwrap();
            // Make sure it's saved in the blobstore
            let root_unode = runtime
                .block_on(Id(unode_id).load(ctx.clone(), &repo.get_blobstore()))
                .unwrap();
            assert_eq!(root_unode.0.parents(), &vec![parent_unode_id]);

            let root_filenode_id = fetch_root_filenode_id(&mut runtime, repo.clone(), bcs_id);
            assert_eq!(
                find_unode_history(&mut runtime, repo.clone(), UnodeEntry::Directory(unode_id)),
                find_filenode_history(&mut runtime, repo.clone(), root_filenode_id),
            );

            let all_unodes = runtime
                .block_on(
                    iterate_all_entries(ctx.clone(), repo.clone(), Entry::Tree(Id(unode_id)))
                        .collect(),
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
    }

    #[test]
    fn test_same_content_different_paths() {
        let repo = linear::getrepo();
        let mut runtime = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock();

        fn check_unode_uniqeness(
            ctx: CoreContext,
            repo: BlobRepo,
            runtime: &mut Runtime,
            file_changes: BTreeMap<MPath, Option<FileChange>>,
        ) {
            let bcs = create_bonsai_changeset(repo.clone(), runtime, file_changes);
            let bcs_id = bcs.get_changeset_id();

            let f = derive_unode_manifest(
                ctx.clone(),
                repo.clone(),
                bcs_id,
                vec![].into_iter(),
                get_file_changes(&bcs),
            );
            let unode_id = runtime.block_on(f).unwrap();

            let unode_mf = runtime
                .block_on(Id(unode_id).load(ctx.clone(), &repo.get_blobstore()))
                .unwrap();

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

    #[test]
    fn test_same_content_no_change() {
        let repo = linear::getrepo();
        let mut runtime = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock();

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

    #[test]
    fn test_parent_order() {
        let repo = new_memblob_empty(None).unwrap();
        let mut runtime = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock();

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
        let bcs = create_bonsai_changeset(repo.clone(), &mut runtime, file_changes);
        let bcs_id = bcs.get_changeset_id();

        let f = derive_unode_manifest(
            ctx.clone(),
            repo.clone(),
            bcs_id,
            vec![p1_root_unode_id, p2_root_unode_id].into_iter(),
            get_file_changes(&bcs),
        );
        let root_unode = runtime.block_on(f).unwrap();

        // Make sure hash is the same if nothing was changed
        let f = derive_unode_manifest(
            ctx.clone(),
            repo.clone(),
            bcs_id,
            vec![p1_root_unode_id, p2_root_unode_id].into_iter(),
            get_file_changes(&bcs),
        );
        let same_root_unode = runtime.block_on(f).unwrap();
        assert_eq!(root_unode, same_root_unode);

        // Now change parent order, make sure hashes are different
        let f = derive_unode_manifest(
            ctx.clone(),
            repo.clone(),
            bcs_id,
            vec![p2_root_unode_id, p1_root_unode_id].into_iter(),
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
        let bcs = create_bonsai_changeset(repo.clone(), &mut runtime, file_changes);

        let bcs_id = bcs.get_changeset_id();
        let f = derive_unode_manifest(
            ctx.clone(),
            repo.clone(),
            bcs_id,
            vec![].into_iter(),
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

        let bcs = create_bonsai_changeset(repo.clone(), &mut runtime, file_changes);
        let first_bcs_id = bcs.get_changeset_id();

        let f = derive_unode_manifest(
            ctx.clone(),
            repo.clone(),
            first_bcs_id,
            vec![].into_iter(),
            get_file_changes(&bcs),
        );
        let first_unode_id = runtime.block_on(f).unwrap();

        let (merge_p1, merge_p1_unode_id) = {
            let file_changes =
                store_files(ctx.clone(), &mut runtime, changes_merge_p1, repo.clone());
            let merge_p1 = create_bonsai_changeset_with_params(
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
                vec![first_unode_id.clone()].into_iter(),
                get_file_changes(&merge_p1),
            );
            let merge_p1_unode_id = runtime.block_on(f).unwrap();
            (merge_p1, merge_p1_unode_id)
        };

        let (merge_p2, merge_p2_unode_id) = {
            let file_changes =
                store_files(ctx.clone(), &mut runtime, changes_merge_p2, repo.clone());

            let merge_p2 = create_bonsai_changeset_with_params(
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
                vec![first_unode_id.clone()].into_iter(),
                get_file_changes(&merge_p2),
            );
            let merge_p2_unode_id = runtime.block_on(f).unwrap();
            (merge_p2, merge_p2_unode_id)
        };

        let file_changes = store_files(ctx.clone(), &mut runtime, changes_merge, repo.clone());
        let merge = create_bonsai_changeset_with_params(
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
            vec![merge_p1_unode_id, merge_p2_unode_id].into_iter(),
            get_file_changes(&merge),
        );
        runtime.block_on(f)
    }

    fn create_bonsai_changeset(
        repo: BlobRepo,
        runtime: &mut Runtime,
        file_changes: BTreeMap<MPath, Option<FileChange>>,
    ) -> BonsaiChangeset {
        create_bonsai_changeset_with_params(repo, runtime, file_changes, "message", vec![])
    }

    fn create_bonsai_changeset_with_params(
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
                CoreContext::test_mock(),
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
                    let content = FileContents::Bytes(Bytes::from(content));
                    let content_id = runtime
                        .block_on(repo.unittest_store(ctx.clone(), content))
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
                UnodeEntry::File(file_unode_id) => Id(file_unode_id.clone())
                    .load(ctx, &repo.get_blobstore())
                    .map(|unode_mf| {
                        unode_mf
                            .0
                            .parents()
                            .into_iter()
                            .cloned()
                            .map(UnodeEntry::File)
                            .collect()
                    })
                    .boxify(),
                UnodeEntry::Directory(mf_unode_id) => Id(mf_unode_id.clone())
                    .load(ctx, &repo.get_blobstore())
                    .map(|unode_mf| {
                        unode_mf
                            .0
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
                UnodeEntry::File(file_unode_id) => Id(file_unode_id.clone())
                    .load(ctx, &repo.get_blobstore())
                    .map(|unode_file| unode_file.0.linknode().clone())
                    .boxify(),
                UnodeEntry::Directory(mf_unode_id) => Id(mf_unode_id.clone())
                    .load(ctx, &repo.get_blobstore())
                    .map(|unode_mf| unode_mf.0.linknode().clone())
                    .boxify(),
            }
        }
    }

    fn find_unode_history(
        runtime: &mut Runtime,
        repo: BlobRepo,
        start: UnodeEntry,
    ) -> Vec<ChangesetId> {
        let ctx = CoreContext::test_mock();
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
        runtime: &mut Runtime,
        repo: BlobRepo,
        start: HgFileNodeId,
    ) -> Vec<ChangesetId> {
        let ctx = CoreContext::test_mock();

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
            let linknode = runtime
                .block_on(repo.get_bonsai_from_hg(ctx.clone(), hg_linknode))
                .unwrap()
                .unwrap();
            history.push(linknode);

            let mf_fut = BlobManifest::load(
                ctx.clone(),
                &repo.get_blobstore(),
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
        runtime: &mut Runtime,
        repo: BlobRepo,
        bcs_id: ChangesetId,
    ) -> HgFileNodeId {
        let ctx = CoreContext::test_mock();
        let hg_cs_id = runtime
            .block_on(repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id))
            .unwrap();
        let hg_cs = runtime
            .block_on(repo.get_changeset_by_changesetid(ctx.clone(), hg_cs_id))
            .unwrap();

        HgFileNodeId::new(hg_cs.manifestid().into_nodehash())
    }
}
