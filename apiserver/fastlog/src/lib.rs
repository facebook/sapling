// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

/// This library is used to efficiently store file and directory history.
/// For each unode we store a FastlogBatch - thrift structure that stores latest commits and their
/// parents that modified this file or directory. Commits are stored in BFS order.
/// All FastlogBatches are stored in blobstore.
///
/// Commits also store pointers to their parents, however they are stored as an offset to the
/// commit hash in batch. I.e. if we have two commits A and B and A is an ancestor of B, then
/// batch will look like:
/// B, vec![ParentOffset(1)]
/// A, vec![]
///
/// Note that commits where a file was deleted are not stored in FastlogBatch. It also doesn't
/// store a history across deletions i.e. if a file was added, then deleted then added again in
/// commit A, FastlogBatch in commit A will contain only one entry.
///
/// RootFastlog is a derived data which derives FastlogBatch for each unode
/// that was created or modified in this commit.
use blobrepo::BlobRepo;
use blobstore::{Blobstore, BlobstoreBytes, Loadable};
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use derive_unode_manifest::derived_data_unodes::{RootUnodeManifestId, RootUnodeManifestMapping};
use derived_data::{BonsaiDerived, BonsaiDerivedMapping};
use failure_ext::{Error, Fail};
use futures::{future, stream::FuturesUnordered, Future, Stream};
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use manifest::{Diff, Entry, ManifestOps};
use mononoke_types::{BonsaiChangeset, ChangesetId, FileUnodeId, ManifestUnodeId};
use std::collections::HashMap;
use std::iter::FromIterator;
use std::sync::Arc;

mod fastlog_impl;
mod thrift {
    pub use mononoke_types_thrift::*;
}

use fastlog_impl::{
    create_new_batch, fetch_fastlog_batch_by_unode_id, fetch_flattened,
    save_fastlog_batch_by_unode_id,
};

/// Returns history for a given unode if it exists.
/// This is the public API of this crate i.e. what clients should use if they want to
/// fetch the history
pub fn prefetch_history(
    ctx: CoreContext,
    repo: BlobRepo,
    unode_entry: Entry<ManifestUnodeId, FileUnodeId>,
) -> impl Future<Item = Option<Vec<(ChangesetId, Vec<FastlogParent>)>>, Error = Error> {
    let blobstore = Arc::new(repo.get_blobstore());
    fetch_fastlog_batch_by_unode_id(ctx.clone(), blobstore.clone(), unode_entry).and_then(
        move |maybe_fastlog_batch| {
            maybe_fastlog_batch.map(|fastlog_batch| fetch_flattened(&fastlog_batch, ctx, blobstore))
        },
    )
}

#[derive(Debug, PartialEq, Eq)]
pub enum FastlogParent {
    /// Parent exists and it's stored in the batch
    Known(ChangesetId),
    /// Parent exists, but it's not stored in the batch (including previous_batches).
    /// It needs to be fetched separately
    Unknown,
}

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "invalid Thrift structure '{}': {}", _0, _1)]
    InvalidThrift(String, String),
    #[fail(display = "Fastlog batch for {:?} unode not found", _0)]
    NotFound(Entry<ManifestUnodeId, FileUnodeId>),
    #[fail(display = "Failed to deserialize FastlogBatch for {}: {}", _0, _1)]
    DeserializationError(String, String),
}

#[derive(Clone)]
pub struct RootFastlog(ChangesetId);

impl BonsaiDerived for RootFastlog {
    const NAME: &'static str = "rootfastlog";

    fn derive_from_parents(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
    ) -> BoxFuture<Self, Error> {
        // TODO(stash): we shouldn't create a RootUnodeManifestMapping mapping here -
        // ideally we should create it once when Mononoke is initialized.
        // But for now this is a limitation of derived data trait - it requires explicit
        // passing of RootUnodeManifestMapping
        let unode_mapping = Arc::new(RootUnodeManifestMapping::new(repo.get_blobstore()));
        let bcs_id = bonsai.get_changeset_id();
        RootUnodeManifestId::derive(ctx.clone(), repo.clone(), unode_mapping.clone(), bcs_id)
            .join(fetch_parent_root_unodes(
                ctx.clone(),
                repo.clone(),
                bonsai,
                unode_mapping,
            ))
            .and_then(move |(root_unode_mf_id, parents)| {
                let blobstore = Arc::new(repo.get_blobstore());
                let unode_mf_id = root_unode_mf_id.manifest_unode_id().clone();

                if parents.len() < 2 {
                    let s = match parents.get(0) {
                        Some(parent) => (*parent)
                            .diff(ctx.clone(), blobstore.clone(), unode_mf_id)
                            .filter_map(|diff_entry| match diff_entry {
                                Diff::Added(_, entry) => Some(entry),
                                Diff::Removed(..) => None,
                                Diff::Changed(_, _, entry) => Some(entry),
                            })
                            .boxify(),
                        None => unode_mf_id
                            .list_all_entries(ctx.clone(), blobstore.clone())
                            .map(|(_, entry)| entry)
                            .boxify(),
                    };

                    s.map(move |entry| {
                        fetch_unode_parents(ctx.clone(), blobstore.clone(), entry).and_then({
                            cloned!(ctx, blobstore);
                            move |parents| {
                                create_new_batch(ctx.clone(), blobstore.clone(), parents, bcs_id)
                                    .and_then({
                                        cloned!(ctx, blobstore);
                                        move |fastlog_batch| {
                                            save_fastlog_batch_by_unode_id(
                                                ctx,
                                                blobstore,
                                                entry,
                                                fastlog_batch,
                                            )
                                        }
                                    })
                            }
                        })
                    })
                    .buffered(100)
                    .collect()
                    .map(move |_| RootFastlog(bcs_id))
                } else {
                    // TODO(stash): handle other cases as well i.e. linear history and history with
                    // merges
                    unimplemented!()
                }
            })
            .boxify()
    }
}

fn fetch_parent_root_unodes(
    ctx: CoreContext,
    repo: BlobRepo,
    bonsai: BonsaiChangeset,
    unode_mapping: Arc<RootUnodeManifestMapping>,
) -> impl Future<Item = Vec<ManifestUnodeId>, Error = Error> {
    let parents: Vec<_> = bonsai.parents().collect();
    future::join_all(parents.into_iter().map(move |p| {
        RootUnodeManifestId::derive(ctx.clone(), repo.clone(), unode_mapping.clone(), p)
            .map(|root_unode_mf_id| root_unode_mf_id.manifest_unode_id().clone())
    }))
}

fn fetch_unode_parents(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    unode_entry_id: Entry<ManifestUnodeId, FileUnodeId>,
) -> impl Future<Item = Vec<Entry<ManifestUnodeId, FileUnodeId>>, Error = Error> {
    unode_entry_id
        .load(ctx, &blobstore)
        .from_err()
        .map(|unode_entry| match unode_entry {
            Entry::Tree(tree) => tree
                .parents()
                .clone()
                .into_iter()
                .map(Entry::Tree)
                .collect(),
            Entry::Leaf(leaf) => leaf
                .parents()
                .clone()
                .into_iter()
                .map(Entry::Leaf)
                .collect(),
        })
}

#[derive(Clone)]
pub struct RootFastlogMapping {
    blobstore: Arc<dyn Blobstore>,
}

impl RootFastlogMapping {
    pub fn new(blobstore: Arc<dyn Blobstore>) -> Self {
        Self { blobstore }
    }

    fn format_key(&self, cs_id: &ChangesetId) -> String {
        format!("derived_rootfastlog.{}", cs_id)
    }
}

impl BonsaiDerivedMapping for RootFastlogMapping {
    type Value = RootFastlog;

    fn get(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<HashMap<ChangesetId, Self::Value>, Error> {
        let gets = csids.into_iter().map(|cs_id| {
            self.blobstore
                .get(ctx.clone(), self.format_key(&cs_id))
                .map(move |maybe_val| maybe_val.map(|_| (cs_id.clone(), RootFastlog(cs_id))))
        });
        FuturesUnordered::from_iter(gets)
            .filter_map(|x| x) // Remove None
            .collect_to()
            .boxify()
    }

    fn put(&self, ctx: CoreContext, csid: ChangesetId, _id: Self::Value) -> BoxFuture<(), Error> {
        self.blobstore.put(
            ctx,
            self.format_key(&csid),
            // Value doesn't matter here, so just put empty Value
            BlobstoreBytes::from_bytes(Bytes::new()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use benchmark_lib::{GenManifest, GenSettings};
    use blobrepo::save_bonsai_changesets;
    use context::CoreContext;
    use fixtures::{
        create_bonsai_changeset, create_bonsai_changeset_with_files, linear, store_files,
    };
    use maplit::btreemap;
    use mercurial_types::HgChangesetId;
    use mononoke_types::{MPath, ManifestUnodeId};
    use pretty_assertions::assert_eq;
    use rand::SeedableRng;
    use rand_xorshift::XorShiftRng;
    use std::collections::{HashSet, VecDeque};
    use std::str::FromStr;
    use tokio::runtime::Runtime;

    #[test]
    fn test_derive_single_empty_commit_no_parents() {
        let mut rt = Runtime::new().unwrap();
        let repo = linear::getrepo();
        let ctx = CoreContext::test_mock();
        let bcs = create_bonsai_changeset(vec![]);
        let bcs_id = bcs.get_changeset_id();
        rt.block_on(save_bonsai_changesets(vec![bcs], ctx.clone(), repo.clone()))
            .unwrap();

        let root_unode_mf_id =
            derive_fastlog_batch_and_unode(&mut rt, ctx.clone(), bcs_id.clone(), repo.clone());

        let list = fetch_list(
            &mut rt,
            ctx.clone(),
            repo.clone(),
            Entry::Tree(root_unode_mf_id),
        );
        assert_eq!(list, vec![(bcs_id, vec![])]);
    }

    #[test]
    fn test_derive_single_commit_no_parents() {
        let mut rt = Runtime::new().unwrap();
        let repo = linear::getrepo();
        let ctx = CoreContext::test_mock();

        // This is the initial diff with no parents
        // See tests/fixtures/src/lib.rs
        let hg_cs_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
        let bcs_id = rt
            .block_on(repo.get_bonsai_from_hg(ctx.clone(), hg_cs_id))
            .unwrap()
            .unwrap();

        let root_unode_mf_id =
            derive_fastlog_batch_and_unode(&mut rt, ctx.clone(), bcs_id.clone(), repo.clone());
        let list = fetch_list(
            &mut rt,
            ctx.clone(),
            repo.clone(),
            Entry::Tree(root_unode_mf_id.clone()),
        );
        assert_eq!(list, vec![(bcs_id, vec![])]);

        let blobstore = Arc::new(repo.get_blobstore());
        let path_1 = MPath::new(&"1").unwrap();
        let path_files = MPath::new(&"files").unwrap();
        let entries = rt
            .block_on(
                root_unode_mf_id
                    .find_entries(ctx.clone(), blobstore.clone(), vec![path_1, path_files])
                    .collect(),
            )
            .unwrap();

        let list = fetch_list(
            &mut rt,
            ctx.clone(),
            repo.clone(),
            entries.get(0).unwrap().1.clone(),
        );
        assert_eq!(list, vec![(bcs_id, vec![])]);

        let list = fetch_list(
            &mut rt,
            ctx.clone(),
            repo.clone(),
            entries.get(1).unwrap().1.clone(),
        );
        assert_eq!(list, vec![(bcs_id, vec![])]);
    }

    #[test]
    fn test_derive_linear() {
        let mut rt = Runtime::new().unwrap();
        let repo = linear::getrepo();
        let ctx = CoreContext::test_mock();

        let hg_cs_id = HgChangesetId::from_str("79a13814c5ce7330173ec04d279bf95ab3f652fb").unwrap();
        let bcs_id = rt
            .block_on(repo.get_bonsai_from_hg(ctx.clone(), hg_cs_id))
            .unwrap()
            .unwrap();

        let root_unode_mf_id =
            derive_fastlog_batch_and_unode(&mut rt, ctx.clone(), bcs_id.clone(), repo.clone());

        let blobstore = Arc::new(repo.get_blobstore());
        let entries = rt
            .block_on(
                root_unode_mf_id
                    .list_all_entries(ctx.clone(), blobstore)
                    .map(|(_, entry)| entry)
                    .collect(),
            )
            .unwrap();

        for entry in entries {
            verify_list(&mut rt, ctx.clone(), repo.clone(), entry);
        }
    }

    #[test]
    fn test_derive_overflow() {
        let mut rt = Runtime::new().unwrap();
        let repo = linear::getrepo();
        let ctx = CoreContext::test_mock();

        let mut bonsais = vec![];
        let mut parents = vec![];
        for i in 1..60 {
            let filename = String::from("1");
            let content = format!("{}", i);
            let stored_files = store_files(
                ctx.clone(),
                btreemap! { filename.as_str() => Some(content.as_str()) },
                repo.clone(),
            );

            let bcs = create_bonsai_changeset_with_files(parents, stored_files);
            let bcs_id = bcs.get_changeset_id();
            bonsais.push(bcs);
            parents = vec![bcs_id];
        }

        let latest = parents.get(0).unwrap();
        rt.block_on(save_bonsai_changesets(bonsais, ctx.clone(), repo.clone()))
            .unwrap();

        verify_all_entries_for_commit(&mut rt, ctx, repo, *latest);
    }

    #[test]
    fn test_random_repo() {
        let mut rt = Runtime::new().unwrap();
        let repo = linear::getrepo();
        let ctx = CoreContext::test_mock();

        let mut rng = XorShiftRng::seed_from_u64(0); // reproducable Rng
        let gen_settings = GenSettings::default();
        let mut changes_count = vec![];
        changes_count.resize(100, 100);
        let latest = rt
            .block_on(GenManifest::new().gen_stack(
                ctx.clone(),
                repo.clone(),
                &mut rng,
                &gen_settings,
                None,
                changes_count,
            ))
            .unwrap();

        verify_all_entries_for_commit(&mut rt, ctx, repo, latest);
    }

    #[test]
    fn test_derive_empty_commits() {
        let mut rt = Runtime::new().unwrap();
        let repo = linear::getrepo();
        let ctx = CoreContext::test_mock();

        let mut bonsais = vec![];
        let mut parents = vec![];
        for _ in 1..60 {
            let bcs = create_bonsai_changeset(parents);
            let bcs_id = bcs.get_changeset_id();
            bonsais.push(bcs);
            parents = vec![bcs_id];
        }

        let latest = parents.get(0).unwrap();
        rt.block_on(save_bonsai_changesets(bonsais, ctx.clone(), repo.clone()))
            .unwrap();

        verify_all_entries_for_commit(&mut rt, ctx, repo, *latest);
    }

    fn verify_all_entries_for_commit(
        rt: &mut Runtime,
        ctx: CoreContext,
        repo: BlobRepo,
        bcs_id: ChangesetId,
    ) {
        let root_unode_mf_id =
            derive_fastlog_batch_and_unode(rt, ctx.clone(), bcs_id.clone(), repo.clone());

        let blobstore = Arc::new(repo.get_blobstore());
        let entries = rt
            .block_on(
                root_unode_mf_id
                    .list_all_entries(ctx.clone(), blobstore.clone())
                    .collect(),
            )
            .unwrap();

        for (path, entry) in entries {
            println!("verifying: path: {:?} unode: {:?}", path, entry);
            verify_list(rt, ctx.clone(), repo.clone(), entry);
        }
    }

    fn derive_fastlog_batch_and_unode(
        rt: &mut Runtime,
        ctx: CoreContext,
        bcs_id: ChangesetId,
        repo: BlobRepo,
    ) -> ManifestUnodeId {
        let blobstore = Arc::new(repo.get_blobstore());
        let mapping = RootFastlogMapping::new(blobstore.clone());
        rt.block_on(RootFastlog::derive(
            ctx.clone(),
            repo.clone(),
            mapping,
            bcs_id,
        ))
        .unwrap();

        let unode_mapping = RootUnodeManifestMapping::new(repo.get_blobstore());
        let root_unode =
            RootUnodeManifestId::derive(ctx.clone(), repo.clone(), Arc::new(unode_mapping), bcs_id);
        let root_unode = rt.block_on(root_unode).unwrap();
        root_unode.manifest_unode_id().clone()
    }

    fn verify_list(
        rt: &mut Runtime,
        ctx: CoreContext,
        repo: BlobRepo,
        entry: Entry<ManifestUnodeId, FileUnodeId>,
    ) {
        let list = fetch_list(rt, ctx.clone(), repo.clone(), entry);
        let actual_bonsais: Vec<_> = list.into_iter().map(|(bcs_id, _)| bcs_id).collect();

        let expected_bonsais = find_unode_history(rt, repo, entry);
        assert_eq!(actual_bonsais, expected_bonsais);
    }

    fn fetch_list(
        rt: &mut Runtime,
        ctx: CoreContext,
        repo: BlobRepo,
        entry: Entry<ManifestUnodeId, FileUnodeId>,
    ) -> Vec<(ChangesetId, Vec<FastlogParent>)> {
        let blobstore = Arc::new(repo.get_blobstore());
        let batch = rt
            .block_on(fetch_fastlog_batch_by_unode_id(
                ctx.clone(),
                blobstore.clone(),
                entry,
            ))
            .unwrap()
            .expect("batch hasn't been generated yet");

        println!(
            "batch for {:?}: latest size: {}, previous batches size: {}",
            entry,
            batch.latest().len(),
            batch.previous_batches().len(),
        );
        assert!(batch.latest().len() <= 10);
        assert!(batch.previous_batches().len() <= 5);
        rt.block_on(fetch_flattened(&batch, ctx, blobstore))
            .unwrap()
    }

    fn find_unode_history(
        runtime: &mut Runtime,
        repo: BlobRepo,
        start: Entry<ManifestUnodeId, FileUnodeId>,
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
            if history.len() >= 60 {
                break;
            }
            let parents = runtime
                .block_on(unode_entry.get_parents(ctx.clone(), repo.clone()))
                .unwrap();
            q.extend(parents.into_iter().filter(|x| visited.insert(x.clone())));
        }

        history
    }

    trait UnodeHistory {
        fn get_parents(
            &self,
            ctx: CoreContext,
            repo: BlobRepo,
        ) -> BoxFuture<Vec<Entry<ManifestUnodeId, FileUnodeId>>, Error>;

        fn get_linknode(&self, ctx: CoreContext, repo: BlobRepo) -> BoxFuture<ChangesetId, Error>;
    }

    impl UnodeHistory for Entry<ManifestUnodeId, FileUnodeId> {
        fn get_parents(
            &self,
            ctx: CoreContext,
            repo: BlobRepo,
        ) -> BoxFuture<Vec<Entry<ManifestUnodeId, FileUnodeId>>, Error> {
            match self {
                Entry::Leaf(file_unode_id) => file_unode_id
                    .load(ctx, &repo.get_blobstore())
                    .from_err()
                    .map(|unode_mf| {
                        unode_mf
                            .parents()
                            .into_iter()
                            .cloned()
                            .map(Entry::Leaf)
                            .collect()
                    })
                    .boxify(),
                Entry::Tree(mf_unode_id) => mf_unode_id
                    .load(ctx, &repo.get_blobstore())
                    .from_err()
                    .map(|unode_mf| {
                        unode_mf
                            .parents()
                            .into_iter()
                            .cloned()
                            .map(Entry::Tree)
                            .collect()
                    })
                    .boxify(),
            }
        }

        fn get_linknode(&self, ctx: CoreContext, repo: BlobRepo) -> BoxFuture<ChangesetId, Error> {
            match self {
                Entry::Leaf(file_unode_id) => file_unode_id
                    .clone()
                    .load(ctx, &repo.get_blobstore())
                    .from_err()
                    .map(|unode_file| unode_file.linknode().clone())
                    .boxify(),
                Entry::Tree(mf_unode_id) => mf_unode_id
                    .load(ctx, &repo.get_blobstore())
                    .from_err()
                    .map(|unode_mf| unode_mf.linknode().clone())
                    .boxify(),
            }
        }
    }
}
