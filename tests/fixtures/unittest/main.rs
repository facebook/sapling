// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Test the linear repo fixture

extern crate ascii;
extern crate futures;

extern crate linear;
extern crate mercurial_types;

use ascii::AsciiString;
use futures::executor::spawn;
use mercurial_types::manifest::{Content, Type};
use mercurial_types::nodehash::{ChangesetId, NodeHash};
use mercurial_types::path::MPath;

#[test]
fn check_heads() {
    let repo = linear::getrepo();

    let heads_stream = repo.get_heads();
    let mut heads = spawn(heads_stream);

    assert!(
        if let Some(Ok(hash)) = heads.wait_stream() {
            hash
                == NodeHash::from_ascii_str(&AsciiString::from_ascii(
                    "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                ).expect("Can't turn string to AsciiString"))
                    .expect(
                    "Can't turn AsciiString to NodeHash",
                )
        } else {
            false
        },
        "Wrong head"
    );

    assert!(
        if let None = heads.wait_stream() {
            true
        } else {
            false
        },
        "Too many heads"
    );
}

#[test]
fn check_head_exists() {
    let repo = linear::getrepo();

    let nodehash =
        NodeHash::from_ascii_str(&AsciiString::from_ascii(
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
        ).expect("Can't turn string to AsciiString"))
            .expect("Can't turn AsciiString to NodeHash");

    let exists_future = repo.changeset_exists(&ChangesetId::new(nodehash));

    let exists = spawn(exists_future)
        .wait_future()
        .expect("Can't determine if changeset exists");
    assert!(exists, "Head is not a valid changeset");
}

#[test]
fn check_head_has_file() {
    let repo = linear::getrepo();

    let changeset_future =
        repo.get_changeset_by_changesetid(&ChangesetId::from_ascii_str(&AsciiString::from_ascii(
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
        ).expect("Can't turn string to AsciiString"))
            .expect("Can't turn AsciiString to NodeHash"));
    let changeset = spawn(changeset_future)
        .wait_future()
        .expect("Can't get changeset");

    let manifest_future =
        repo.get_manifest_by_nodeid(&changeset.manifestid().clone().into_nodehash());
    let manifest = spawn(manifest_future)
        .wait_future()
        .expect("Can't get manifest");

    let files_future = manifest.lookup(&MPath::new(b"files").expect("Can't get file 'files'"));
    let files = spawn(files_future)
        .wait_future()
        .expect("Can't fetch file")
        .expect("Can't read file");
    assert!(files.get_type() == Type::File);
    let content_future = files.get_content();
    let content = spawn(content_future)
        .wait_future()
        .expect("Can't get file content");
    if let Content::File(blob) = content {
        assert_eq!(blob.size(), Some(21));
        assert_eq!(
            blob.into_inner().expect("Can't read content"),
            b"1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n"
        );
    } else {
        panic!("files is not a file blob");
    }
}

#[test]
fn count_changesets() {
    let repo = linear::getrepo();
    let all_changesets_stream = repo.get_changesets();
    let mut all_changesets = spawn(all_changesets_stream);
    let mut count = 0;
    loop {
        let item = all_changesets.wait_stream();
        if let None = item {
            break;
        } else {
            count += 1;
        }
    }
    assert_eq!(count, 10);
}
