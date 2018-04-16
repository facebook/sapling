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
extern crate dbbookmarks;
extern crate dieselfilenodes;
extern crate mercurial_types;
extern crate memheads;
extern crate blobrepo;
extern crate blobstore;
extern crate ascii;
extern crate heads;
extern crate futures;
extern crate bytes;
#[macro_use]
extern crate slog;

use std::str::FromStr;

use bytes::Bytes;
use changesets::{Changesets, ChangesetInsert, SqliteChangesets};
use memblob::EagerMemblob;
use dbbookmarks::SqliteDbBookmarks;
use dieselfilenodes::SqliteFilenodes;
use mercurial_types::{DChangesetId, DNodeHash, RepositoryId};
use memheads::MemHeads;
use blobrepo::BlobRepo;
use ascii::AsciiString;
use blobstore::Blobstore;
use heads::Heads;
use futures::future::Future;
use slog::{Discard, Drain, Logger};
use std::sync::Arc;

pub fn getrepo(logger: Option<Logger>) -> BlobRepo {
    let bookmarks = Arc::new(SqliteDbBookmarks::in_memory()
        .expect("cannot create in-memory bookmarks table"));
    let heads = Arc::new(MemHeads::new());
    let blobs = Arc::new(EagerMemblob::new());
    let filenodes = Arc::new(SqliteFilenodes::in_memory()
        .expect("cannot create in-memory filenodes"));
    let changesets = Arc::new(SqliteChangesets::in_memory()
        .expect("cannot create in-memory changeset table"));
    let repo_id = RepositoryId::new(0);

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
        with open(os.path.join(args.source, "topology")) as f:
            for line in f.readlines():
                line = line.strip()
                split = line.split(' ')
                if len(split) == 0:
                    raise Exception("Incorrect commit graph topology")

                commit_hash = split[0]
                writeline(
                    'let cs_id = DChangesetId::new(DNodeHash::from_str("{}").unwrap());'.
                    format(commit_hash)
                )
                writeline('let parents = vec![')
                if len(split) > 1:
                    indent += 1
                    for p in split[1:-1]:
                        writeline(
                            'DChangesetId::new(DNodeHash::from_str("{}").unwrap()), '.
                            format(p)
                        )

                    writeline(
                        'DChangesetId::new(DNodeHash::from_str("{}").unwrap())'.
                        format(split[-1])
                    )
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

        for head in glob.glob(os.path.join(args.source, "heads", "head-*")):
            head = head[-40:]
            writeline(
                'heads.add(&DNodeHash::from_ascii_str(&AsciiString::from_ascii("{}").unwrap()).unwrap()).wait().expect("Head put failed");'.
                format(head)
            )
        writeline("")
        blob_prefix_len = len(os.path.join(args.source, "blobs", "blob-"))
        for blob in glob.glob(os.path.join(args.source, "blobs", "blob-*")):
            key = blob[blob_prefix_len:]
            with open(blob, "rb") as data:
                blobdata = "\\x".join(chunk_string(data.read().hex()))
                writeline(
                    'blobs.put(String::from("{}"), Bytes::from_static(b"\\x{}")).wait().expect("Blob put failed");'.
                    format(key, blobdata)
                )
        rs.writelines(
            """
    let logger = logger.unwrap_or(Logger::root(Discard {}.ignore_res(), o!()));
    BlobRepo::new(logger, heads, bookmarks, blobs, filenodes, changesets, repo_id)
}
"""
        )
