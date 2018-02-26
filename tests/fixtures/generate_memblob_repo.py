#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All Rights Reserved.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import argparse
import glob
import os
import shutil


def parse_args():
    parser = argparse.ArgumentParser(
        description="Generate a memblob repo rust source"
    )
    parser.add_argument("--install_dir")
    parser.add_argument("source")
    return parser.parse_args()


def chunk_string(s):
    for start in range(0, len(s), 2):
        yield s[start:start + 2]


if __name__ == '__main__':
    args = parse_args()
    shutil.copytree(args.source, os.path.join(args.install_dir, args.source))
    os.chdir(args.install_dir)
    with open(os.path.join(args.install_dir, "lib.rs"), "w") as rs:
        rs.writelines(
            """
// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate changesets;
extern crate memblob;
extern crate membookmarks;
extern crate mercurial_types;
extern crate memheads;
extern crate memlinknodes;
extern crate blobrepo;
extern crate blobstore;
extern crate ascii;
extern crate heads;
extern crate futures;
extern crate bytes;

use bytes::Bytes;
use changesets::SqliteChangesets;
use memblob::EagerMemblob;
use membookmarks::MemBookmarks;
use mercurial_types::{NodeHash, RepositoryId};
use memheads::MemHeads;
use memlinknodes::MemLinknodes;
use blobrepo::BlobRepo;
use ascii::AsciiString;
use blobstore::Blobstore;
use heads::Heads;
use futures::future::Future;

pub fn getrepo() -> BlobRepo {
    let bookmarks: MemBookmarks = MemBookmarks::new();
    let heads: MemHeads = MemHeads::new();
    let blobs = EagerMemblob::new();
    let linknodes = MemLinknodes::new();
    let changesets = SqliteChangesets::in_memory()
        .expect("cannot create in-memory changeset table");

"""
        )
        for head in glob.glob(os.path.join(args.source, "heads", "head-*")):
            head = head[-40:]
            rs.write(
                '    heads.add(&NodeHash::from_ascii_str(&AsciiString::from_ascii("{}").unwrap()).unwrap()).wait().expect("Head put failed");\n'.
                format(head)
            )
        rs.write("\n")
        blob_prefix_len = len(os.path.join(args.source, "blobs", "blob-"))
        for blob in glob.glob(os.path.join(args.source, "blobs", "blob-*")):
            key = blob[blob_prefix_len:]
            with open(blob, "rb") as data:
                blobdata = "\\x".join(chunk_string(data.read().hex()))
                rs.write(
                    '    blobs.put(String::from("{}"), Bytes::from_static(b"\\x{}")).wait().expect("Blob put failed");\n'.
                    format(key, blobdata)
                )
        for linknode in glob.glob(
            os.path.join(args.source, "linknodes", "linknode-*")
        ):
            with open(linknode, "rb") as data:
                linknode_data = "\\x".join(chunk_string(data.read().hex()))
                rs.write(
                    '    linknodes.add_data_encoded(&b"\\x{}"[..]).expect("Linknode add failed");\n'.
                    format(linknode_data)
                )
        rs.writelines(
            """
    BlobRepo::new_memblob(heads, bookmarks, blobs, linknodes, changesets, RepositoryId::new(0))
}
"""
        )
