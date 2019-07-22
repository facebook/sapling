// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use blobstore::{Blobstore, Loadable};
use cloned::cloned;
use context::CoreContext;
use failure_ext::{Error, Fail};
use futures::{future, Future};
use futures_ext::{BoxFuture, FutureExt};
use manifest::{derive_manifest, Entry, LeafInfo, Manifest, TreeInfo};
use mononoke_types::unode::{FileUnode, ManifestUnode, UnodeEntry};
use mononoke_types::{
    BlobstoreValue, ChangesetId, ContentId, FileType, FileUnodeId, ManifestUnodeId, MononokeId,
};
use mononoke_types::{MPath, MPathElement};
use repo_blobstore::RepoBlobstore;
use std::collections::BTreeMap;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "cannot fetch ManifestUnode: {}", _0)]
    FailFetchManifestUnode(ManifestUnodeId),
}

// Id type is added so that we can implement Loadable trait for ManifestUnodeId
// We can't do it now because ManifestUnodeId and Loadable are defined in different crates.
// TODO(stash): move Loadable trait implementation into mononoke_types?
#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct Id<T>(T);

impl Loadable for Id<ManifestUnodeId> {
    type Value = Id<ManifestUnode>;

    fn load(
        &self,
        ctx: CoreContext,
        blobstore: impl Blobstore + Clone,
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
    // TODO(stash): handle merges correctly
    // In particular if one parent is ancestor of another parent then it should not be
    // a parent of this manifest unode
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
    let LeafInfo {
        leaf,
        path,
        parents,
    } = leaf_info;

    if let Some((content_id, file_type)) = leaf {
        let file_unode = FileUnode::new(
            parents,
            content_id,
            file_type,
            path.get_path_hash(),
            linknode,
        );
        let file_unode_id = file_unode.get_unode_id();
        return blobstore
            .put(
                ctx,
                file_unode_id.blobstore_key(),
                file_unode.into_blob().into(),
            )
            .map(move |()| file_unode_id)
            .boxify();
    }

    // TODO(stash): implement merges
    unimplemented!()
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
    use blobrepo::{save_bonsai_changesets, BlobManifest};
    use bytes::Bytes;
    use failure_ext::err_msg;
    use fixtures::linear;
    use futures::stream::{self, Stream};
    use futures_ext::bounded_traversal::bounded_traversal_stream;
    use manifest::Manifest;
    use maplit::btreemap;
    use mercurial_types::{Changeset, HgChangesetId, HgFileNodeId, HgManifestId};
    use mononoke_types::{
        BonsaiChangeset, BonsaiChangesetMut, DateTime, FileChange, FileContents, RepoPath,
    };
    use std::collections::{HashSet, VecDeque};
    use std::str::FromStr;
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
                get_changes(&bcs),
            );

            let unode_id = runtime.block_on(f).unwrap();
            // Make sure it's saved in the blobstore
            runtime
                .block_on(Id(unode_id).load(ctx.clone(), repo.get_blobstore()))
                .unwrap();
            let all_unodes = iterate_all_unodes(
                ctx.clone(),
                repo.clone(),
                &mut runtime,
                UnodeEntry::Directory(unode_id),
            );
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
                get_changes(&bcs),
            );

            let unode_id = runtime.block_on(f).unwrap();
            // Make sure it's saved in the blobstore
            let root_unode = runtime
                .block_on(Id(unode_id).load(ctx.clone(), repo.get_blobstore()))
                .unwrap();
            assert_eq!(root_unode.0.parents(), &vec![parent_unode_id]);

            let root_filenode_id = fetch_root_filenode_id(&mut runtime, repo.clone(), bcs_id);
            assert_eq!(
                find_unode_history(&mut runtime, repo.clone(), UnodeEntry::Directory(unode_id)),
                find_filenode_history(&mut runtime, repo.clone(), root_filenode_id),
            );

            let all_unodes = iterate_all_unodes(
                ctx.clone(),
                repo.clone(),
                &mut runtime,
                UnodeEntry::Directory(unode_id),
            );
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
            let bcs = BonsaiChangesetMut {
                parents: vec![],
                author: "author".to_string(),
                author_date: DateTime::now(),
                committer: None,
                committer_date: None,
                message: "message".to_string(),
                extra: btreemap! {},
                file_changes,
            }
            .freeze()
            .unwrap();

            let bcs_id = bcs.get_changeset_id();
            save_bonsai_changesets(vec![bcs.clone()], ctx.clone(), repo.clone())
                .wait()
                .unwrap();

            let f = derive_unode_manifest(
                ctx.clone(),
                repo.clone(),
                bcs_id,
                vec![].into_iter(),
                get_changes(&bcs),
            );
            let unode_id = runtime.block_on(f).unwrap();

            let unode_mf = runtime
                .block_on(Id(unode_id).load(ctx.clone(), repo.get_blobstore()))
                .unwrap();

            // Unodes should be unique even if content is the same. Check it
            let vals: Vec<_> = unode_mf.list().collect();
            assert_eq!(vals.len(), 2);
            assert_ne!(vals.get(0), vals.get(1));
        }

        let file_changes = store_files(
            ctx.clone(),
            &mut runtime,
            btreemap! {"file1" => Some("content"), "file2" => Some("content")},
            repo.clone(),
        );
        check_unode_uniqeness(ctx.clone(), repo.clone(), &mut runtime, file_changes);

        let file_changes = store_files(
            ctx.clone(),
            &mut runtime,
            btreemap! {"dir1/file" => Some("content"), "dir2/file" => Some("content")},
            repo.clone(),
        );
        check_unode_uniqeness(ctx.clone(), repo.clone(), &mut runtime, file_changes);
    }

    fn iterate_all_unodes(
        ctx: CoreContext,
        repo: BlobRepo,
        runtime: &mut Runtime,
        unode_entry: UnodeEntry,
    ) -> Vec<(Option<MPath>, UnodeEntry)> {
        let blobstore = repo.get_blobstore();
        let walk_future: BoxFuture<_, Error> =
            bounded_traversal_stream(256, (None, unode_entry), move |(path, unode_entry)| {
                match unode_entry {
                    UnodeEntry::File(unode_file_id) => Id(unode_file_id)
                        .load(ctx.clone(), blobstore.clone())
                        .map(move |_| (vec![(path, unode_entry)], vec![]))
                        .left_future(),
                    UnodeEntry::Directory(unode_mf_id) => Id(unode_mf_id)
                        .load(ctx.clone(), blobstore.clone())
                        .map(move |mf| {
                            let recurse = mf
                                .0
                                .list()
                                .map(|(basename, entry)| {
                                    let path = MPath::join_opt_element(path.as_ref(), &basename);
                                    (Some(path), entry.clone())
                                })
                                .collect();
                            (vec![(path, unode_entry)], recurse)
                        })
                        .right_future(),
                }
            })
            .map(|entries| stream::iter_ok(entries))
            .flatten()
            .collect()
            .boxify();

        runtime.block_on(walk_future).unwrap()
    }

    fn store_files(
        ctx: CoreContext,
        runtime: &mut Runtime,
        files: BTreeMap<&str, Option<&str>>,
        repo: BlobRepo,
    ) -> BTreeMap<MPath, Option<FileChange>> {
        let mut res = btreemap! {};

        for (path, content) in files {
            let path = MPath::new(path).unwrap();
            match content {
                Some(content) => {
                    let size = content.len();
                    let content = FileContents::Bytes(Bytes::from(content));
                    let content_id = runtime
                        .block_on(repo.unittest_store(ctx.clone(), content))
                        .unwrap();

                    let file_change =
                        FileChange::new(content_id, FileType::Regular, size as u64, None);
                    res.insert(path, Some(file_change));
                }
                None => {
                    res.insert(path, None);
                }
            }
        }
        res
    }

    impl Loadable for Id<FileUnodeId> {
        type Value = Id<FileUnode>;

        fn load(
            &self,
            ctx: CoreContext,
            blobstore: impl Blobstore + Clone,
        ) -> BoxFuture<Self::Value, Error> {
            let unode_id = self.0;
            blobstore
                .get(ctx, unode_id.blobstore_key())
                .and_then(move |bytes| match bytes {
                    None => Err(err_msg("failed to fetch filenode")),
                    Some(bytes) => FileUnode::from_bytes(bytes.as_bytes().as_ref()).map(Id),
                })
                .boxify()
        }
    }

    fn get_changes(
        bcs: &BonsaiChangeset,
    ) -> impl IntoIterator<Item = (MPath, Option<(ContentId, FileType)>)> {
        let v: Vec<_> = bcs
            .file_changes()
            .map(|(mpath, maybe_file_change)| {
                let content_file_type = match maybe_file_change {
                    Some(file_change) => Some((file_change.content_id(), file_change.file_type())),
                    None => None,
                };
                (mpath.clone(), content_file_type)
            })
            .collect();
        v.into_iter()
    }

    fn get_bonsai_changeset(
        ctx: CoreContext,
        repo: BlobRepo,
        runtime: &mut Runtime,
        s: &str,
    ) -> (ChangesetId, BonsaiChangeset) {
        let hg_cs_id = HgChangesetId::from_str(s).unwrap();

        let bcs_id = runtime
            .block_on(repo.get_bonsai_from_hg(ctx.clone(), hg_cs_id))
            .unwrap()
            .unwrap();
        let bcs = runtime
            .block_on(repo.get_bonsai_changeset(ctx.clone(), bcs_id))
            .unwrap();
        (bcs_id, bcs)
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
                    .load(ctx, repo.get_blobstore())
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
                    .load(ctx, repo.get_blobstore())
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
                    .load(ctx, repo.get_blobstore())
                    .map(|unode_file| unode_file.0.linknode().clone())
                    .boxify(),
                UnodeEntry::Directory(mf_unode_id) => Id(mf_unode_id.clone())
                    .load(ctx, repo.get_blobstore())
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
