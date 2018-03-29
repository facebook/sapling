// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::BTreeMap;

use ascii::AsAsciiStr;
use bytes::Bytes;
use failure::Error;
use futures::executor::spawn;
use futures::future::Future;
use futures::stream::futures_unordered;
use futures_ext::{BoxFuture, StreamExt};

use blobrepo::{BlobEntry, BlobRepo, ChangesetHandle};
use memblob::{EagerMemblob, LazyMemblob};
use mercurial_types::{manifest, Blob, NodeHash, RepoPath, Time};
use std::sync::Arc;

pub fn get_empty_eager_repo() -> BlobRepo {
    BlobRepo::new_memblob_empty(None, Some(Arc::new(EagerMemblob::new())))
        .expect("cannot create empty repo")
}

pub fn get_empty_lazy_repo() -> BlobRepo {
    BlobRepo::new_memblob_empty(None, Some(Arc::new(LazyMemblob::new())))
        .expect("cannot create empty repo")
}

macro_rules! test_both_repotypes {
    ($impl_name:ident, $lazy_test:ident, $eager_test:ident) => {
        #[test]
        fn $lazy_test() {
            async_unit::tokio_unit_test(|| {
                $impl_name(get_empty_lazy_repo());
            })
        }

        #[test]
        fn $eager_test() {
            async_unit::tokio_unit_test(|| {
                $impl_name(get_empty_eager_repo());
            })
        }
    };
    (should_panic, $impl_name:ident, $lazy_test:ident, $eager_test:ident) => {
        #[test]
        #[should_panic]
        fn $lazy_test() {
            async_unit::tokio_unit_test(|| {
                $impl_name(get_empty_lazy_repo());
            })
        }

        #[test]
        #[should_panic]
        fn $eager_test() {
            async_unit::tokio_unit_test(|| {
                $impl_name(get_empty_eager_repo());
            })
        }
    }
}

pub fn upload_file_no_parents<S>(
    repo: &BlobRepo,
    data: S,
    path: &RepoPath,
) -> (NodeHash, BoxFuture<(BlobEntry, RepoPath), Error>)
where
    S: Into<String>,
{
    let blob: Blob = Bytes::from(data.into().as_bytes()).into();
    repo.upload_entry(blob, manifest::Type::File, None, None, path.clone())
        .unwrap()
}

pub fn upload_file_one_parent<S>(
    repo: &BlobRepo,
    data: S,
    path: &RepoPath,
    p1: NodeHash,
) -> (NodeHash, BoxFuture<(BlobEntry, RepoPath), Error>)
where
    S: Into<String>,
{
    let blob: Blob = Bytes::from(data.into().as_bytes()).into();
    repo.upload_entry(blob, manifest::Type::File, Some(p1), None, path.clone())
        .unwrap()
}

pub fn upload_manifest_no_parents<S>(
    repo: &BlobRepo,
    data: S,
    path: &RepoPath,
) -> (NodeHash, BoxFuture<(BlobEntry, RepoPath), Error>)
where
    S: Into<String>,
{
    let blob: Blob = Bytes::from(data.into().as_bytes()).into();
    repo.upload_entry(blob, manifest::Type::Tree, None, None, path.clone())
        .unwrap()
}

pub fn upload_manifest_one_parent<S>(
    repo: &BlobRepo,
    data: S,
    path: &RepoPath,
    p1: NodeHash,
) -> (NodeHash, BoxFuture<(BlobEntry, RepoPath), Error>)
where
    S: Into<String>,
{
    let blob: Blob = Bytes::from(data.into().as_bytes()).into();
    repo.upload_entry(blob, manifest::Type::Tree, Some(p1), None, path.clone())
        .unwrap()
}

pub fn create_changeset_no_parents(
    repo: &BlobRepo,
    root_manifest: BoxFuture<(BlobEntry, RepoPath), Error>,
    other_nodes: Vec<BoxFuture<(BlobEntry, RepoPath), Error>>,
) -> ChangesetHandle {
    repo.create_changeset(
        None,
        None,
        root_manifest,
        futures_unordered(other_nodes).boxify(),
        "author <author@fb.com>".into(),
        Time { time: 0, tz: 0 },
        BTreeMap::new(),
        "Test commit".into(),
    )
}

pub fn create_changeset_one_parent(
    repo: &BlobRepo,
    root_manifest: BoxFuture<(BlobEntry, RepoPath), Error>,
    other_nodes: Vec<BoxFuture<(BlobEntry, RepoPath), Error>>,
    p1: ChangesetHandle,
) -> ChangesetHandle {
    repo.create_changeset(
        Some(p1),
        None,
        root_manifest,
        futures_unordered(other_nodes).boxify(),
        "\u{041F}\u{0451}\u{0442}\u{0440} <peter@fb.com>".into(),
        Time { time: 1234, tz: 0 },
        BTreeMap::new(),
        "Child commit".into(),
    )
}

pub fn string_to_nodehash(hash: &str) -> NodeHash {
    NodeHash::from_ascii_str(hash.as_ascii_str().unwrap()).unwrap()
}

pub fn run_future<F>(future: F) -> Result<F::Item, F::Error>
where
    F: Future,
{
    spawn(future).wait_future()
}
