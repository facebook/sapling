/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use ascii::AsciiString;
use blobrepo::BlobRepo;
use blobstore::{Blobstore, Loadable};
use cloned::cloned;
use context::CoreContext;
use crypto::digest::Digest;
use failure_ext::{err_msg, Error, FutureFailureExt};
use filestore::{get_metadata, FetchKey};
use futures::{future, stream, sync::mpsc, Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, FutureExt};
use manifest::{derive_manifest, Entry, LeafInfo, TreeInfo};
use mononoke_types::fsnode::{Fsnode, FsnodeDirectory, FsnodeEntry, FsnodeFile, FsnodeSummary};
use mononoke_types::hash::{Sha1, Sha256};
use mononoke_types::{BlobstoreValue, ContentId, ContentMetadata, FileType, FsnodeId, MononokeId};
use mononoke_types::{MPath, MPathElement};
use repo_blobstore::RepoBlobstore;

use crate::ErrorKind;

/// Derives fsnodes for bonsai_changeset `cs_id` given parent fsnodes. Note
/// that `derive_manifest()` does a lot of the heavy lifting for us, and this
/// crate has to provide only functions to create a single fsnode, and check
/// that the leaf entries (which are `(ContentId, FileType)` pairs) are valid
/// during merges.
pub(crate) fn derive_fsnode(
    ctx: CoreContext,
    repo: BlobRepo,
    parents: Vec<FsnodeId>,
    changes: Vec<(MPath, Option<(ContentId, FileType)>)>,
) -> impl Future<Item = FsnodeId, Error = Error> {
    future::lazy(move || {
        let blobstore = repo.get_blobstore();
        let (sender, receiver) = mpsc::unbounded();
        let content_ids = changes
            .iter()
            .filter_map(|(_mpath, content_id_and_file_type)| {
                content_id_and_file_type.map(|(content_id, _file_type)| content_id)
            })
            .collect();
        prefetch_content_metadata(ctx.clone(), blobstore.clone(), content_ids)
            .map(Arc::new)
            .and_then(move |prefetched_content_metadata| {
                derive_manifest(
                    ctx.clone(),
                    blobstore.clone(),
                    parents.clone(),
                    changes,
                    {
                        cloned!(blobstore, ctx, sender, prefetched_content_metadata);
                        move |tree_info| {
                            create_fsnode(
                                ctx.clone(),
                                blobstore.clone(),
                                sender.clone(),
                                prefetched_content_metadata.clone(),
                                tree_info,
                            )
                        }
                    },
                    check_fsnode_leaf,
                )
                .and_then(move |maybe_tree_id| match maybe_tree_id {
                    Some(tree_id) => future::ok(tree_id).left_future(),
                    None => {
                        // All files have been deleted, generate empty fsnode
                        let tree_info = TreeInfo {
                            path: None,
                            parents,
                            subentries: Default::default(),
                        };
                        create_fsnode(
                            ctx,
                            blobstore,
                            sender,
                            prefetched_content_metadata,
                            tree_info,
                        )
                        .map(|(_, tree_id)| tree_id)
                        .right_future()
                    }
                })
            })
            .join(
                receiver
                    .map_err(|()| err_msg("receiver failed"))
                    .buffered(1024)
                    .for_each(|_| Ok(())),
            )
            .map(|(result, ())| result)
    })
}

// Prefetch metadata for all content IDs introduced by a changeset.
fn prefetch_content_metadata(
    ctx: CoreContext,
    blobstore: RepoBlobstore,
    content_ids: HashSet<ContentId>,
) -> impl Future<Item = HashMap<ContentId, ContentMetadata>, Error = Error> {
    stream::futures_unordered(content_ids.into_iter().map({
        cloned!(blobstore, ctx);
        move |content_id| {
            get_metadata(&blobstore, ctx.clone(), &FetchKey::Canonical(content_id))
                .map(move |metadata| (content_id, metadata))
        }
    }))
    .collect()
    .map(|metadata| {
        metadata
            .into_iter()
            .filter_map(|(content_id, metadata)| metadata.map(|metadata| (content_id, metadata)))
            .collect::<HashMap<_, _>>()
    })
}

/// Collect all the subentries for a new fsnode, re-using entries the parent
/// fsnodes to avoid fetching too much.
fn collect_fsnode_subentries(
    ctx: CoreContext,
    blobstore: RepoBlobstore,
    prefetched_content_metadata: Arc<HashMap<ContentId, ContentMetadata>>,
    parents: Vec<FsnodeId>,
    subentries: BTreeMap<
        MPathElement,
        (
            Option<Option<FsnodeSummary>>,
            Entry<FsnodeId, (ContentId, FileType)>,
        ),
    >,
) -> impl Future<Item = Vec<(MPathElement, FsnodeEntry)>, Error = Error> {
    // Load the parent fsnodes
    stream::futures_unordered(parents.into_iter().map({
        cloned!(ctx, blobstore);
        move |fsnode_id| {
            fsnode_id
                .load(ctx.clone(), &blobstore)
                .context(ErrorKind::MissingParent(fsnode_id))
        }
    }))
    .collect()
    .from_err()
    .and_then({
        cloned!(ctx, blobstore);
        move |parent_fsnodes| {
            // Collect all entries from the parent fsnodes as a cache.
            let mut file_cache = HashMap::new();
            let mut dir_cache = HashMap::new();
            for parent_fsnode in parent_fsnodes.into_iter() {
                for (_elem, entry) in parent_fsnode.list() {
                    match entry {
                        FsnodeEntry::File(file) => {
                            file_cache
                                .entry((*file.content_id(), *file.file_type()))
                                .or_insert(file.clone());
                        }
                        FsnodeEntry::Directory(dir) => {
                            dir_cache.entry(*dir.id()).or_insert(dir.clone());
                        }
                    }
                }
            }

            // Find or fetch the `FsnodeEntry` for each of the subentries.
            stream::futures_ordered(subentries.into_iter().map(move |(elem, (summary, entry))| {
                match entry {
                    Entry::Tree(fsnode_id) => {
                        if let Some(Some(summary)) = summary {
                            // The subdirectory was just created. Use the
                            // summary we just calculated.
                            future::ok((
                                elem.clone(),
                                FsnodeEntry::Directory(FsnodeDirectory::new(fsnode_id, summary)),
                            ))
                            .boxify()
                        } else if let Some(entry) = dir_cache.get(&fsnode_id) {
                            // The subdirectory was already in this
                            // directory. Use the cached entry.
                            future::ok((elem.clone(), FsnodeEntry::Directory(entry.clone())))
                                .boxify()
                        } else {
                            // Some other directory is being used. Fetch its
                            // summary from the blobstore.
                            fsnode_id
                                .load(ctx.clone(), &blobstore)
                                .with_context({
                                    cloned!(elem);
                                    move || {
                                        ErrorKind::MissingSubentry(
                                            String::from_utf8_lossy(elem.as_ref()).to_string(),
                                            fsnode_id,
                                        )
                                    }
                                })
                                .from_err()
                                .map({
                                    cloned!(elem, fsnode_id);
                                    move |fsnode| {
                                        let entry = FsnodeEntry::Directory(FsnodeDirectory::new(
                                            fsnode_id,
                                            fsnode.summary().clone(),
                                        ));
                                        (elem, entry)
                                    }
                                })
                                .boxify()
                        }
                    }
                    Entry::Leaf(content_id_and_file_type) => {
                        if let Some(entry) = file_cache.get(&content_id_and_file_type) {
                            // The file was already in this directory. Use
                            // the cached entry.
                            future::ok((elem.clone(), FsnodeEntry::File(entry.clone()))).boxify()
                        } else {
                            // Some other file is being used. Use the
                            // metadata we prefetched to create a new entry.
                            let (content_id, file_type) = content_id_and_file_type.clone();
                            if let Some(metadata) = prefetched_content_metadata.get(&content_id) {
                                let entry = FsnodeEntry::File(FsnodeFile::new(
                                    content_id,
                                    file_type,
                                    metadata.total_size,
                                    metadata.sha1,
                                    metadata.sha256,
                                ));
                                future::ok((elem.clone(), entry)).boxify()
                            } else {
                                future::err(ErrorKind::MissingContent(content_id).into()).boxify()
                            }
                        }
                    }
                }
            }))
            .collect()
        }
    })
}

/// Create a new fsnode for the tree described by `tree_info`.
fn create_fsnode(
    ctx: CoreContext,
    blobstore: RepoBlobstore,
    sender: mpsc::UnboundedSender<BoxFuture<(), Error>>,
    prefetched_content_metadata: Arc<HashMap<ContentId, ContentMetadata>>,
    tree_info: TreeInfo<FsnodeId, (ContentId, FileType), Option<FsnodeSummary>>,
) -> impl Future<Item = (Option<FsnodeSummary>, FsnodeId), Error = Error> {
    collect_fsnode_subentries(
        ctx.clone(),
        blobstore.clone(),
        prefetched_content_metadata,
        tree_info.parents,
        tree_info.subentries,
    )
    .and_then(move |entries| {
        // Build a summary of the entries and store it as the new fsnode.
        let entries: BTreeMap<_, _> = entries.into_iter().collect();
        let simple_format_sha1 = {
            let mut bytes = [0; 20];
            let mut digest = generate_simple_format_digest(
                crypto::sha1::Sha1::new(),
                &entries,
                |fsnode_file| fsnode_file.content_sha1().to_hex(),
                |fsnode_dir| fsnode_dir.summary().simple_format_sha1.to_hex(),
            );
            digest.result(&mut bytes);
            Sha1::from_byte_array(bytes)
        };
        let simple_format_sha256 = {
            let mut bytes = [0; 32];
            let mut digest = generate_simple_format_digest(
                crypto::sha2::Sha256::new(),
                &entries,
                |fsnode_file| fsnode_file.content_sha256().to_hex(),
                |fsnode_dir| fsnode_dir.summary().simple_format_sha256.to_hex(),
            );
            digest.result(&mut bytes);
            Sha256::from_byte_array(bytes)
        };
        let mut summary = FsnodeSummary {
            simple_format_sha1,
            simple_format_sha256,
            child_files_count: 0,
            child_files_total_size: 0,
            child_dirs_count: 0,
            descendant_files_count: 0,
            descendant_files_total_size: 0,
        };
        for (_elem, entry) in entries.iter() {
            match entry {
                FsnodeEntry::File(fsnode_file) => {
                    let size = fsnode_file.size();
                    summary.child_files_count += 1;
                    summary.child_files_total_size += size;
                    summary.descendant_files_count += 1;
                    summary.descendant_files_total_size += size;
                }
                FsnodeEntry::Directory(fsnode_dir) => {
                    let subdir_summary = fsnode_dir.summary();
                    summary.child_dirs_count += 1;
                    summary.descendant_files_count += subdir_summary.descendant_files_count;
                    summary.descendant_files_total_size +=
                        subdir_summary.descendant_files_total_size;
                }
            }
        }
        let fsnode = Fsnode::new(entries, summary.clone());
        let fsnode_id = fsnode.get_fsnode_id();
        let key = fsnode_id.blobstore_key();
        let blob = fsnode.into_blob();
        let f = blobstore.put(ctx, key, blob.into()).boxify();

        sender
            .unbounded_send(f)
            .into_future()
            .map(move |()| (Some(summary), fsnode_id))
            .map_err(|err| err_msg(format!("failed to send fsnode future {}", err)))
    })
}

/// Generate the simple format hash for a directory. See
/// `mononoke_types/src/fsnodes.rs` for a description of what the simple
/// format is.
fn generate_simple_format_digest<H, F, D>(
    mut digest: H,
    dir: &BTreeMap<MPathElement, FsnodeEntry>,
    get_file_hash: F,
    get_dir_hash: D,
) -> H
where
    H: Digest,
    F: Fn(&FsnodeFile) -> AsciiString,
    D: Fn(&FsnodeDirectory) -> AsciiString,
{
    for (elem, entry) in dir.iter() {
        match entry {
            FsnodeEntry::File(file) => {
                digest.input(get_file_hash(&file).as_bytes());
                digest.input(match file.file_type() {
                    FileType::Regular => b" file ",
                    FileType::Executable => b" exec ",
                    FileType::Symlink => b" link ",
                });
            }
            FsnodeEntry::Directory(dir) => {
                digest.input(get_dir_hash(&dir).as_bytes());
                digest.input(b" tree ");
            }
        }
        digest.input(elem.as_ref());
        digest.input(b"\0");
    }
    digest
}

/// There are no leaves stored for fsnodes, however we still need to check
/// that any merge operations have resulted in valid fsnodes, where, for each
/// file, either all the parents have the same file contents, or the
/// changeset includes a change for that file.
fn check_fsnode_leaf(
    leaf_info: LeafInfo<(ContentId, FileType), (ContentId, FileType)>,
) -> impl Future<Item = (Option<FsnodeSummary>, (ContentId, FileType)), Error = Error> {
    if let Some(content_id_and_file_type) = leaf_info.leaf {
        future::ok((None, content_id_and_file_type))
    } else {
        // This bonsai changeset is a merge. If all content IDs and file
        // types match for this file, then the content ID is valid. Check
        // this is so.
        if leaf_info.parents.len() < 2 {
            return future::err(
                ErrorKind::InvalidBonsai(
                    "no change is provided, but file has only one parent".to_string(),
                )
                .into(),
            );
        }
        let mut iter = leaf_info.parents.clone().into_iter();
        let content_id_and_file_type = iter.next().and_then(|first_elem| {
            if iter.all(|next_elem| next_elem == first_elem) {
                Some(first_elem)
            } else {
                None
            }
        });
        if let Some(content_id_and_file_type) = content_id_and_file_type {
            future::ok((None, content_id_and_file_type))
        } else {
            future::err(
                ErrorKind::InvalidBonsai(
                    "no change is provided, but file content or type is different".to_string(),
                )
                .into(),
            )
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::mapping::get_file_changes;
    use fbinit::FacebookInit;
    use fixtures::{linear, many_files_dirs};
    use std::str::FromStr;
    use test_utils::{get_bonsai_changeset, iterate_all_entries};
    use tokio::runtime::Runtime;

    #[fbinit::test]
    fn flat_linear_test(fb: FacebookInit) {
        let repo = linear::getrepo(fb);
        let mut runtime = Runtime::new().unwrap();

        let ctx = CoreContext::test_mock(fb);
        let parent_fsnode_id = {
            let parent_hg_cs = "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536";
            let (_bcs_id, bcs) =
                get_bonsai_changeset(ctx.clone(), repo.clone(), &mut runtime, parent_hg_cs);

            let f = derive_fsnode(ctx.clone(), repo.clone(), vec![], get_file_changes(&bcs));

            let root_fsnode_id = runtime.block_on(f).unwrap();

            // Make sure it's saved in the blobstore.
            let root_fsnode = runtime
                .block_on(root_fsnode_id.load(ctx.clone(), &repo.get_blobstore()))
                .unwrap();

            // Make sure the fsnodes describe the full manifest.
            let all_fsnodes: BTreeMap<_, _> = runtime
                .block_on(
                    iterate_all_entries(ctx.clone(), repo.clone(), Entry::Tree(root_fsnode_id))
                        .collect(),
                )
                .unwrap()
                .into_iter()
                .collect();
            assert_eq!(
                all_fsnodes.keys().collect::<Vec<_>>(),
                vec![
                    &None,
                    &Some(MPath::new("1").unwrap()),
                    &Some(MPath::new("files").unwrap())
                ]
            );

            // Make sure the root fsnode is correct.
            assert_eq!(
                root_fsnode.summary(),
                &FsnodeSummary {
                    simple_format_sha1: Sha1::from_str("b23c1ffbd30ffa92942ec67748ea28084bbbb100")
                        .unwrap(),
                    simple_format_sha256: Sha256::from_str(
                        "6639d6877355e0b26b135cc0bf90b8cb901c85f43e71fbf4e8a5202ff1a7f868"
                    )
                    .unwrap(),
                    child_files_count: 2,
                    child_files_total_size: 4,
                    child_dirs_count: 0,
                    descendant_files_count: 2,
                    descendant_files_total_size: 4,
                }
            );
            root_fsnode_id
        };

        {
            let child_hg_cs = "3e0e761030db6e479a7fb58b12881883f9f8c63f";
            let (_bcs_id, bcs) =
                get_bonsai_changeset(ctx.clone(), repo.clone(), &mut runtime, child_hg_cs);

            let f = derive_fsnode(
                ctx.clone(),
                repo.clone(),
                vec![parent_fsnode_id.clone()],
                get_file_changes(&bcs),
            );

            let root_fsnode_id = runtime.block_on(f).unwrap();

            // Make sure it's saved in the blobstore
            let root_fsnode = runtime
                .block_on(root_fsnode_id.load(ctx.clone(), &repo.get_blobstore()))
                .unwrap();

            // Make sure the fsnodes describe the full manifest.
            let all_fsnodes: BTreeMap<_, _> = runtime
                .block_on(
                    iterate_all_entries(ctx.clone(), repo.clone(), Entry::Tree(root_fsnode_id))
                        .collect(),
                )
                .unwrap()
                .into_iter()
                .collect();
            assert_eq!(
                all_fsnodes.keys().collect::<Vec<_>>(),
                vec![
                    &None,
                    &Some(MPath::new("1").unwrap()),
                    &Some(MPath::new("2").unwrap()),
                    &Some(MPath::new("files").unwrap())
                ]
            );

            // Make sure the root fsnode is correct.
            assert_eq!(
                root_fsnode.summary(),
                &FsnodeSummary {
                    simple_format_sha1: Sha1::from_str("e1c394ce3584f6c69680807b93997e46099e3f84")
                        .unwrap(),
                    simple_format_sha256: Sha256::from_str(
                        "bae96aa31c752e678f22bd1d051ba0a2def7992cf6857c1a1736fd6517106c08"
                    )
                    .unwrap(),
                    child_files_count: 3,
                    child_files_total_size: 8,
                    child_dirs_count: 0,
                    descendant_files_count: 3,
                    descendant_files_total_size: 8,
                }
            );
        }
    }

    #[fbinit::test]
    fn nested_directories_test(fb: FacebookInit) {
        let repo = many_files_dirs::getrepo(fb);
        let mut runtime = Runtime::new().unwrap();

        let ctx = CoreContext::test_mock(fb);

        // Derive fsnodes for the first two commits.  We will look at commit 3 and 4.
        let parent_fsnode_id = {
            let parent_hg_cs = "5a28e25f924a5d209b82ce0713d8d83e68982bc8";
            let (_bcs_id, bcs) =
                get_bonsai_changeset(ctx.clone(), repo.clone(), &mut runtime, parent_hg_cs);
            let f = derive_fsnode(ctx.clone(), repo.clone(), vec![], get_file_changes(&bcs));
            runtime.block_on(f).unwrap()
        };

        let parent_fsnode_id = {
            let parent_hg_cs = "2f866e7e549760934e31bf0420a873f65100ad63";
            let (_bcs_id, bcs) =
                get_bonsai_changeset(ctx.clone(), repo.clone(), &mut runtime, parent_hg_cs);
            let f = derive_fsnode(
                ctx.clone(),
                repo.clone(),
                vec![parent_fsnode_id.clone()],
                get_file_changes(&bcs),
            );
            runtime.block_on(f).unwrap()
        };

        let parent_fsnode_id = {
            let parent_hg_cs = "d261bc7900818dea7c86935b3fb17a33b2e3a6b4";
            let (_bcs_id, bcs) =
                get_bonsai_changeset(ctx.clone(), repo.clone(), &mut runtime, parent_hg_cs);

            let f = derive_fsnode(
                ctx.clone(),
                repo.clone(),
                vec![parent_fsnode_id.clone()],
                get_file_changes(&bcs),
            );

            let root_fsnode_id = runtime.block_on(f).unwrap();

            // Make sure it's saved in the blobstore.
            let root_fsnode = runtime
                .block_on(root_fsnode_id.load(ctx.clone(), &repo.get_blobstore()))
                .unwrap();

            // Make sure the fsnodes describe the full manifest.
            let all_fsnodes: BTreeMap<_, _> = runtime
                .block_on(
                    iterate_all_entries(ctx.clone(), repo.clone(), Entry::Tree(root_fsnode_id))
                        .collect(),
                )
                .unwrap()
                .into_iter()
                .collect();
            assert_eq!(
                all_fsnodes.keys().collect::<Vec<_>>(),
                vec![
                    &None,
                    &Some(MPath::new("1").unwrap()),
                    &Some(MPath::new("2").unwrap()),
                    &Some(MPath::new("dir1").unwrap()),
                    &Some(MPath::new("dir1/file_1_in_dir1").unwrap()),
                    &Some(MPath::new("dir1/file_2_in_dir1").unwrap()),
                    &Some(MPath::new("dir1/subdir1").unwrap()),
                    &Some(MPath::new("dir1/subdir1/file_1").unwrap()),
                    &Some(MPath::new("dir1/subdir1/subsubdir1").unwrap()),
                    &Some(MPath::new("dir1/subdir1/subsubdir1/file_1").unwrap()),
                    &Some(MPath::new("dir1/subdir1/subsubdir2").unwrap()),
                    &Some(MPath::new("dir1/subdir1/subsubdir2/file_1").unwrap()),
                    &Some(MPath::new("dir1/subdir1/subsubdir2/file_2").unwrap()),
                    &Some(MPath::new("dir2").unwrap()),
                    &Some(MPath::new("dir2/file_1_in_dir2").unwrap()),
                ]
            );

            // Make sure the root fsnode is correct.  Test the simple format hashes are as expected.
            let simple_format_sha1 = {
                let text = concat!(
                    "e5fa44f2b31c1fb553b6021e7360d07d5d91ff5e file 1\0",
                    "7448d8798a4380162d4b56f9b452e2f6f9e24e7a file 2\0",
                    "be0dface7b74d8a69b39fb4691ef9eee36077ede tree dir1\0",
                    "ad02b5a5f778d9ad6afd42fcc8e0b889254b5215 tree dir2\0",
                );
                let mut digest = crypto::sha1::Sha1::new();
                digest.input_str(&text);
                let mut bytes = [0; 20];
                digest.result(&mut bytes);
                Sha1::from_byte_array(bytes)
            };
            let simple_format_sha256 = {
                let text = concat!(
                    "4355a46b19d348dc2f57c046f8ef63d4538ebb936000f3c9ee954a27460dd865 file 1\0",
                    "53c234e5e8472b6ac51c1ae1cab3fe06fad053beb8ebfd8977b010655bfdd3c3 file 2\0",
                    "eebf7e41f348db6b31c11fe7adf577bd0951300436a1fd37e53d628127b3517e tree dir1\0",
                    "583c3d388efb78eb9dec46626662d6657bb53706c1ee10770c0fb3e859bd36e1 tree dir2\0",
                );
                let mut digest = crypto::sha2::Sha256::new();
                digest.input_str(&text);
                let mut bytes = [0; 32];
                digest.result(&mut bytes);
                Sha256::from_byte_array(bytes)
            };

            assert_eq!(
                root_fsnode.summary(),
                &FsnodeSummary {
                    simple_format_sha1,
                    simple_format_sha256,
                    child_files_count: 2,
                    child_files_total_size: 4,
                    child_dirs_count: 2,
                    descendant_files_count: 9,
                    descendant_files_total_size: 67,
                }
            );

            // Check one of the deeper fsnodes.
            let deep_fsnode_id = match all_fsnodes.get(&Some(MPath::new("dir1/subdir1").unwrap())) {
                Some(Entry::Tree(fsnode_id)) => fsnode_id,
                _ => panic!("dir1/subdir1 fsnode should be a tree"),
            };
            let deep_fsnode = runtime
                .block_on(deep_fsnode_id.load(ctx.clone(), &repo.get_blobstore()))
                .unwrap();
            let deep_fsnode_entries: Vec<_> = deep_fsnode.list().collect();
            assert_eq!(
                deep_fsnode_entries,
                vec![
                    (
                        &MPathElement::new(b"file_1".to_vec()).unwrap(),
                        &FsnodeEntry::File(FsnodeFile::new(
                            ContentId::from_str(
                                "271119c630193464c55cf7feb72c16dd18b953e65ee85047d4d956381cdb96d9",
                            )
                            .unwrap(),
                            FileType::Regular,
                            9,
                            Sha1::from_str("eba7b799b1e587506677c108eaa51ca273ddcfdc").unwrap(),
                            Sha256::from_str(
                                "8bfab644c313b0227b1862786426697a9b5283eea8eb6066cc8d8e134ce0daa6",
                            )
                            .unwrap(),
                        )),
                    ),
                    (
                        &MPathElement::new(b"subsubdir1".to_vec()).unwrap(),
                        &FsnodeEntry::Directory(FsnodeDirectory::new(
                            FsnodeId::from_str(
                                "a8deafcaa9fdafad84447d754b1e24140f1113e68b3f7ec17c55a4fd9c90fdc7",
                            )
                            .unwrap(),
                            FsnodeSummary {
                                simple_format_sha1: Sha1::from_str(
                                    "b58f168a508c4388f496b21acee468b13b89ac50",
                                )
                                .unwrap(),
                                simple_format_sha256: Sha256::from_str(
                                    "dc19c9a55b59d97dbb2912e5acb1bad17c1aa2835c4cf288f2d47bb0b0b539bc",
                                )
                                .unwrap(),
                                child_files_count: 1,
                                child_files_total_size: 9,
                                child_dirs_count: 0,
                                descendant_files_count: 1,
                                descendant_files_total_size: 9,
                            },
                        )),
                    ),
                    (
                        &MPathElement::new(b"subsubdir2".to_vec()).unwrap(),
                        &FsnodeEntry::Directory(FsnodeDirectory::new(
                            FsnodeId::from_str(
                                "3daadf48916e4b25e28e21a15de4352188154fcaa749207eb00fb72dbd812343",
                            )
                            .unwrap(),
                            FsnodeSummary {
                                simple_format_sha1: Sha1::from_str(
                                    "89babd6eab901598b3df0a0c3318b83bd2edfa4b",
                                )
                                .unwrap(),
                                simple_format_sha256: Sha256::from_str(
                                    "5fa3074b0a96f8ac8f5ab31266d1ce95b1c84a35e8b3f4b8108385881e9e9f15",
                                )
                                .unwrap(),
                                child_files_count: 2,
                                child_files_total_size: 18,
                                child_dirs_count: 0,
                                descendant_files_count: 2,
                                descendant_files_total_size: 18,
                            },
                        )),
                    ),
                ]
            );
            assert_eq!(
                deep_fsnode.summary(),
                &FsnodeSummary {
                    simple_format_sha1: Sha1::from_str("56daa486e90686b5ead6f3ce360a6cbb7b73f23f")
                        .unwrap(),
                    simple_format_sha256: Sha256::from_str(
                        "8c24cf135f74933775b0ac1453c127c987874beb6fde5454ccf9c6c8c726e032"
                    )
                    .unwrap(),
                    child_files_count: 1,
                    child_files_total_size: 9,
                    child_dirs_count: 2,
                    descendant_files_count: 4,
                    descendant_files_total_size: 36,
                }
            );

            root_fsnode_id
        };

        {
            let child_hg_cs = "051946ed218061e925fb120dac02634f9ad40ae2";
            let (_bcs_id, bcs) =
                get_bonsai_changeset(ctx.clone(), repo.clone(), &mut runtime, child_hg_cs);

            let f = derive_fsnode(
                ctx.clone(),
                repo.clone(),
                vec![parent_fsnode_id.clone()],
                get_file_changes(&bcs),
            );

            let root_fsnode_id = runtime.block_on(f).unwrap();

            // Make sure it's saved in the blobstore
            let root_fsnode = runtime
                .block_on(root_fsnode_id.load(ctx.clone(), &repo.get_blobstore()))
                .unwrap();

            // Make sure the fsnodes describe the full manifest.
            let all_fsnodes: BTreeMap<_, _> = runtime
                .block_on(
                    iterate_all_entries(ctx.clone(), repo.clone(), Entry::Tree(root_fsnode_id))
                        .collect(),
                )
                .unwrap()
                .into_iter()
                .collect();
            assert_eq!(
                all_fsnodes.keys().collect::<Vec<_>>(),
                vec![
                    &None,
                    &Some(MPath::new("1").unwrap()),
                    &Some(MPath::new("2").unwrap()),
                    &Some(MPath::new("dir1").unwrap()),
                    &Some(MPath::new("dir2").unwrap()),
                    &Some(MPath::new("dir2/file_1_in_dir2").unwrap()),
                ]
            );

            // Make sure the root fsnode is correct.
            assert_eq!(
                root_fsnode.summary(),
                &FsnodeSummary {
                    simple_format_sha1: Sha1::from_str("753b41c7bf23bda7eabb71095867c2ddf3e485df")
                        .unwrap(),
                    simple_format_sha256: Sha256::from_str(
                        "d86a29b0e92bcbff179e20a27fde18aacedc225a6c4884a7760d2880794c9e51"
                    )
                    .unwrap(),
                    child_files_count: 3,
                    child_files_total_size: 16,
                    child_dirs_count: 1,
                    descendant_files_count: 4,
                    descendant_files_total_size: 25,
                }
            );
        }
    }
}
