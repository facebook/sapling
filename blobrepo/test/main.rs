// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate ascii;
extern crate bytes;
extern crate futures;

extern crate blobrepo;
extern crate changesets;
extern crate memblob;
extern crate membookmarks;
extern crate memheads;
extern crate memlinknodes;
extern crate mercurial_types;

use ascii::AsAsciiStr;
use bytes::Bytes;
use futures::executor::spawn;
use futures::future::Future;

use blobrepo::BlobRepo;
use changesets::SqliteChangesets;
use memblob::EagerMemblob;
use membookmarks::MemBookmarks;
use memheads::MemHeads;
use memlinknodes::MemLinknodes;
use mercurial_types::{manifest, Blob, Entry, EntryId, MPathElement, NodeHash, RepoPath};

fn get_empty_repo() -> BlobRepo {
    let bookmarks: MemBookmarks = MemBookmarks::new();
    let heads: MemHeads = MemHeads::new();
    let blobs = EagerMemblob::new();
    let linknodes = MemLinknodes::new();
    let changesets = SqliteChangesets::in_memory().expect("cannot create in memory changesets");

    BlobRepo::new_memblob(heads, bookmarks, blobs, linknodes, changesets)
}

#[test]
fn upload_blob_no_parents() {
    let repo = get_empty_repo();

    let blob: Blob<Bytes> = Bytes::from(&b"blob"[..]).into();
    let expected_hash = NodeHash::from_ascii_str(
        "c3127cdbf2eae0f09653f9237d85c8436425b246"
            .as_ascii_str()
            .unwrap(),
    ).unwrap();
    let fake_path = RepoPath::file("fake/file").expect("Can't generate fake RepoPath");

    // The blob does not exist...
    assert!(
        spawn(repo.get_file_content(&expected_hash))
            .wait_future()
            .is_err()
    );

    // We upload it...
    let (hash, future) =
        repo.upload_entry(blob, manifest::Type::File, None, None, fake_path.clone())
            .unwrap();
    assert!(hash == expected_hash);

    // The entry we're given is correct...
    assert!(
        spawn(
            future
                .and_then(|(entry, path)| {
                    assert!(path == fake_path);
                    assert!(entry.get_hash() == &EntryId::new(expected_hash));
                    assert!(entry.get_type() == manifest::Type::File);
                    assert!(entry.get_name() == &Some(MPathElement::new("file".into())));
                    entry.get_content()
                })
                .map(|content| match content {
                    manifest::Content::File(f) => assert!(f == b"blob"[..].into()),
                    _ => panic!(),
                }),
        ).wait_future()
            .is_ok()
    );

    // And the blob now exists
    assert!(
        spawn(
            repo.get_file_content(&expected_hash)
                .map(|bytes| assert!(bytes == b"blob"))
        ).wait_future()
            .is_ok()
    );
}

#[test]
fn upload_blob_one_parent() {
    let repo = get_empty_repo();

    let blob: Blob<Bytes> = Bytes::from(&b"blob"[..]).into();
    let expected_hash = NodeHash::from_ascii_str(
        "c2d60b35a8e7e034042a9467783bbdac88a0d219"
            .as_ascii_str()
            .unwrap(),
    ).unwrap();
    let fake_path = RepoPath::file("fake/file").expect("Can't generate fake RepoPath");

    let (p1, future) = repo.upload_entry(
        blob.clone(),
        manifest::Type::File,
        None,
        None,
        fake_path.clone(),
    ).unwrap();

    // The blob does not exist...
    assert!(
        spawn(repo.get_file_content(&expected_hash))
            .wait_future()
            .is_err()
    );

    // We upload it...
    let future = future.map(|_| {
        repo.upload_entry(
            blob,
            manifest::Type::File,
            Some(p1),
            None,
            fake_path.clone(),
        )
    });
    let (hash, future) = spawn(future).wait_future().unwrap().unwrap();

    assert!(hash == expected_hash);

    // The entry we're given is correct...
    assert!(
        spawn(
            future
                .and_then(|(entry, path)| {
                    assert!(path == fake_path);
                    assert!(entry.get_hash() == &EntryId::new(expected_hash));
                    assert!(entry.get_type() == manifest::Type::File);
                    assert!(entry.get_name() == &Some(MPathElement::new("file".into())));
                    entry.get_content()
                })
                .map(|content| match content {
                    manifest::Content::File(f) => assert!(f == b"blob"[..].into()),
                    _ => panic!(),
                }),
        ).wait_future()
            .is_ok()
    );

    // And the blob now exists
    assert!(
        spawn(
            repo.get_file_content(&expected_hash)
                .map(|bytes| assert!(bytes == b"blob"))
        ).wait_future()
            .is_ok()
    );
}
