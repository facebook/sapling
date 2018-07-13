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

#![deny(warnings)]

extern crate bookmarks;
extern crate bonsai_hg_mapping;
extern crate changesets;
extern crate dbbookmarks;
extern crate dieselfilenodes;
extern crate mercurial_types;
extern crate mononoke_types;
extern crate blobrepo;
extern crate blobstore;
extern crate ascii;
extern crate futures;
extern crate bytes;
#[macro_use]
extern crate slog;

use std::str::FromStr;

use ascii::AsciiString;
use bookmarks::{Bookmark, Bookmarks};
use bonsai_hg_mapping::SqliteBonsaiHgMapping;
use changesets::{Changesets, ChangesetInsert, SqliteChangesets};
use dbbookmarks::SqliteDbBookmarks;
use dieselfilenodes::SqliteFilenodes;
use mercurial_types::{HgChangesetId, HgNodeHash, RepositoryId};
use mononoke_types::BlobstoreBytes;
use blobrepo::BlobRepo;
use blobstore::{Blobstore, EagerMemblob};
use futures::future::Future;
use slog::{Discard, Drain, Logger};
use std::sync::Arc;

pub fn getrepo(logger: Option<Logger>) -> BlobRepo {
    let bookmarks = Arc::new(SqliteDbBookmarks::in_memory()
        .expect("cannot create in-memory bookmarks table"));
    let blobs = Arc::new(EagerMemblob::new());
    let filenodes = Arc::new(SqliteFilenodes::in_memory()
        .expect("cannot create in-memory filenodes"));
    let changesets = Arc::new(SqliteChangesets::in_memory()
        .expect("cannot create in-memory changeset table"));
    let bonsai_hg_mapping = Arc::new(SqliteBonsaiHgMapping::in_memory()
        .expect("cannot create in-memory bonsai_hg_mapping table"));
    let repo_id = RepositoryId::new(1);
    let mut book_txn = bookmarks.create_transaction(&repo_id);

"""
        )
        indent = 0

        def writeline(line):
            if line == "":
                rs.write("")
            else:
                rs.write(' ' * 4 * indent)
                rs.write(line)
                rs.write('\n')

        indent += 1
        heads = set()
        with open(os.path.join(args.source, "topology")) as f:
            for line in f.readlines():
                line = line.strip()
                split = line.split(' ')
                if len(split) == 0:
                    raise Exception("Incorrect commit graph topology")

                commit_hash = split[0]
                writeline(
                    'let cs_id = HgChangesetId::new(HgNodeHash::from_str("{}").unwrap());'.
                    format(commit_hash)
                )
                heads.add(commit_hash)

                writeline('let parents = vec![')
                if len(split) > 1:
                    indent += 1
                    for p in split[1:]:
                        writeline(
                            'HgChangesetId::new(HgNodeHash::from_str("{}").unwrap()),'.
                            format(p)
                        )
                        # Any hashes that are parents aren't heads.
                        heads.discard(p)
                    indent -= 1
                writeline('];')

                writeline('let cs_insert = ChangesetInsert {')
                indent += 1
                writeline('repo_id,')
                writeline('cs_id,')
                writeline('parents,')
                indent -= 1
                writeline('};')
                writeline(
                    "changesets.add(cs_insert.clone()).map_err(move |err| panic!(\"changsets {:?} failed {:?}\", cs_insert, err)).wait().expect(\"changesets.add failed\");"
                )
                writeline("")
            writeline("")

        # TODO: (rain1) T30397209 Dump and replay the bookmarks sqlite table
        # instead: https://stackoverflow.com/q/6677540
        for head in heads:
            writeline(
                '''book_txn.create(
                    &Bookmark::new("head-{0}".to_string()).unwrap(),
                    &HgChangesetId::new(HgNodeHash::from_ascii_str(
                        &AsciiString::from_ascii("{0}").unwrap(),
                    ).unwrap()),
                ).expect("Bookmark creation failed");'''.format(head)
            )
        writeline("")

        blob_prefix_len = len(os.path.join(args.source, "blobs", "blob-"))
        for blob in glob.glob(os.path.join(args.source, "blobs", "blob-*")):
            key = blob[blob_prefix_len:]
            with open(blob, "rb") as data:
                blobdata = "\\x".join(chunk_string(data.read().hex()))
                writeline(
                    'blobs.put(String::from("{}"), BlobstoreBytes::from_bytes(&b"\\x{}"[..])).wait().expect("Blob put failed");'.
                    format(key, blobdata)
                )
        rs.writelines(
            """
    book_txn.commit().wait().expect("Bookmark heads creation failed");
    let logger = logger.unwrap_or(Logger::root(Discard {}.ignore_res(), o!()));
    BlobRepo::new(
        logger,
        bookmarks,
        blobs,
        filenodes,
        changesets,
        bonsai_hg_mapping,
        repo_id,
    )
}
"""
        )
