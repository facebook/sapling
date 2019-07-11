// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate blobrepo;
extern crate bytes;
extern crate context;
extern crate futures;
#[macro_use]
extern crate maplit;
extern crate mononoke_types;

use blobrepo::{save_bonsai_changesets, BlobRepo};
use bytes::Bytes;
use context::CoreContext;
use futures::future::Future;
use mononoke_types::{
    BonsaiChangesetMut, ChangesetId, DateTime, FileChange, FileContents, FileType, MPath,
};

use std::collections::BTreeMap;

pub fn store_files(
    ctx: CoreContext,
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
                let content_id = repo.unittest_store(ctx.clone(), content).wait().unwrap();

                let file_change = FileChange::new(content_id, FileType::Regular, size as u64, None);
                res.insert(path, Some(file_change));
            }
            None => {
                res.insert(path, None);
            }
        }
    }
    res
}

pub fn store_rename(
    ctx: CoreContext,
    copy_src: (MPath, ChangesetId),
    path: &str,
    content: &str,
    repo: BlobRepo,
) -> (MPath, Option<FileChange>) {
    let path = MPath::new(path).unwrap();
    let size = content.len();
    let content = FileContents::Bytes(Bytes::from(content));
    let content_id = repo.unittest_store(ctx, content).wait().unwrap();

    let file_change = FileChange::new(content_id, FileType::Regular, size as u64, Some(copy_src));
    (path, Some(file_change))
}

pub fn create_commit(
    ctx: CoreContext,
    repo: BlobRepo,
    parents: Vec<ChangesetId>,
    file_changes: BTreeMap<MPath, Option<FileChange>>,
) -> ChangesetId {
    let bcs = BonsaiChangesetMut {
        parents,
        author: "author".to_string(),
        author_date: DateTime::from_timestamp(0, 0).unwrap(),
        committer: None,
        committer_date: None,
        message: "message".to_string(),
        extra: btreemap! {},
        file_changes,
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx, repo.clone())
        .wait()
        .unwrap();
    bcs_id
}

pub fn create_commit_with_date(
    ctx: CoreContext,
    repo: BlobRepo,
    parents: Vec<ChangesetId>,
    file_changes: BTreeMap<MPath, Option<FileChange>>,
    author_date: DateTime,
) -> ChangesetId {
    let bcs = BonsaiChangesetMut {
        parents,
        author: "author".to_string(),
        author_date,
        committer: None,
        committer_date: None,
        message: "message".to_string(),
        extra: btreemap! {},
        file_changes,
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx, repo.clone())
        .wait()
        .unwrap();
    bcs_id
}
