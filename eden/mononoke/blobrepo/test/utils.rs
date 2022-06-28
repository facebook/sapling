/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use anyhow::Error;
use ascii::AsAsciiStr;
use bytes::Bytes;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryFutureExt;
use scuba_ext::MononokeScubaSampleBuilder;

use ::manifest::Entry;
use blobrepo::BlobRepo;
use blobrepo_hg::ChangesetHandle;
use blobrepo_hg::CreateChangeset;
use context::CoreContext;
use mercurial_types::blobs::ChangesetMetadata;
use mercurial_types::blobs::UploadHgFileContents;
use mercurial_types::blobs::UploadHgFileEntry;
use mercurial_types::blobs::UploadHgNodeHash;
use mercurial_types::blobs::UploadHgTreeEntry;
use mercurial_types::HgBlobNode;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mercurial_types::MPath;
use mercurial_types::RepoPath;
use mononoke_types::DateTime;

pub fn upload_file_no_parents<B>(
    ctx: CoreContext,
    repo: &BlobRepo,
    data: B,
    path: &RepoPath,
) -> (
    HgFileNodeId,
    BoxFuture<'static, Result<(HgFileNodeId, RepoPath), Error>>,
)
where
    B: Into<Bytes>,
{
    upload_hg_file_entry(ctx, repo, data.into(), path.clone(), None, None)
}

pub fn upload_file_one_parent<B>(
    ctx: CoreContext,
    repo: &BlobRepo,
    data: B,
    path: &RepoPath,
    p1: HgFileNodeId,
) -> (
    HgFileNodeId,
    BoxFuture<'static, Result<(HgFileNodeId, RepoPath), Error>>,
)
where
    B: Into<Bytes>,
{
    upload_hg_file_entry(ctx, repo, data.into(), path.clone(), Some(p1), None)
}

pub fn upload_manifest_no_parents<B>(
    ctx: CoreContext,
    repo: &BlobRepo,
    data: B,
    path: &RepoPath,
) -> (
    HgManifestId,
    BoxFuture<'static, Result<(HgManifestId, RepoPath), Error>>,
)
where
    B: Into<Bytes>,
{
    upload_hg_tree_entry(ctx, repo, data.into(), path.clone(), None, None)
}

pub fn upload_manifest_one_parent<B>(
    ctx: CoreContext,
    repo: &BlobRepo,
    data: B,
    path: &RepoPath,
    p1: HgManifestId,
) -> (
    HgManifestId,
    BoxFuture<'static, Result<(HgManifestId, RepoPath), Error>>,
)
where
    B: Into<Bytes>,
{
    upload_hg_tree_entry(ctx, repo, data.into(), path.clone(), Some(p1), None)
}

fn upload_hg_tree_entry(
    ctx: CoreContext,
    repo: &BlobRepo,
    contents: Bytes,
    path: RepoPath,
    p1: Option<HgManifestId>,
    p2: Option<HgManifestId>,
) -> (
    HgManifestId,
    BoxFuture<'static, Result<(HgManifestId, RepoPath), Error>>,
) {
    let upload = UploadHgTreeEntry {
        upload_node_id: UploadHgNodeHash::Generate,
        contents,
        p1: p1.map(|mfid| mfid.into_nodehash()),
        p2: p2.map(|mfid| mfid.into_nodehash()),
        path,
    };
    let (hgcsid, fut) = upload.upload(ctx, repo.blobstore().boxed()).unwrap();
    (hgcsid, fut.compat().boxed())
}

fn upload_hg_file_entry(
    ctx: CoreContext,
    repo: &BlobRepo,
    contents: Bytes,
    path: RepoPath,
    p1: Option<HgFileNodeId>,
    p2: Option<HgFileNodeId>,
) -> (
    HgFileNodeId,
    BoxFuture<'static, Result<(HgFileNodeId, RepoPath), Error>>,
) {
    // Ideally the node id returned from upload.upload would be used, but that isn't immediately
    // available -- so compute it ourselves.
    let node_id = HgBlobNode::new(
        contents.clone(),
        p1.map(HgFileNodeId::into_nodehash),
        p2.map(HgFileNodeId::into_nodehash),
    )
    .nodeid();

    let upload = UploadHgFileEntry {
        upload_node_id: UploadHgNodeHash::Checked(node_id),
        contents: UploadHgFileContents::RawBytes(contents, repo.filestore_config()),
        p1,
        p2,
    };
    let path = path.into_mpath().expect("expected a path to be present");

    let upload_fut = upload.upload_with_path(ctx, repo.get_blobstore().boxed(), path);
    (HgFileNodeId::new(node_id), upload_fut.boxed())
}

pub fn create_changeset_no_parents(
    fb: FacebookInit,
    repo: &BlobRepo,
    root_manifest: BoxFuture<'static, Result<Option<(HgManifestId, RepoPath)>, Error>>,
    other_nodes: Vec<
        BoxFuture<'static, Result<(Entry<HgManifestId, HgFileNodeId>, RepoPath), Error>>,
    >,
) -> ChangesetHandle {
    let cs_metadata = ChangesetMetadata {
        user: "author <author@fb.com>".into(),
        time: DateTime::from_timestamp(0, 0).expect("valid timestamp"),
        extra: BTreeMap::new(),
        message: "Test commit".into(),
    };
    let create_changeset = CreateChangeset {
        expected_nodeid: None,
        expected_files: None,
        p1: None,
        p2: None,
        root_manifest,
        sub_entries: other_nodes
            .into_iter()
            .collect::<FuturesUnordered<_>>()
            .boxed(),
        cs_metadata,
        create_bonsai_changeset_hook: None,
        scribe_category: None,
    };
    create_changeset.create(
        CoreContext::test_mock(fb),
        repo,
        MononokeScubaSampleBuilder::with_discard(),
    )
}

pub fn create_changeset_one_parent(
    fb: FacebookInit,
    repo: &BlobRepo,
    root_manifest: BoxFuture<'static, Result<Option<(HgManifestId, RepoPath)>, Error>>,
    other_nodes: Vec<
        BoxFuture<'static, Result<(Entry<HgManifestId, HgFileNodeId>, RepoPath), Error>>,
    >,
    p1: ChangesetHandle,
) -> ChangesetHandle {
    let cs_metadata = ChangesetMetadata {
        user: "\u{041F}\u{0451}\u{0442}\u{0440} <peter@fb.com>".into(),
        time: DateTime::from_timestamp(1234, 0).expect("valid timestamp"),
        extra: BTreeMap::new(),
        message: "Child commit".into(),
    };
    let create_changeset = CreateChangeset {
        expected_nodeid: None,
        expected_files: None,
        p1: Some(p1),
        p2: None,
        root_manifest: root_manifest.boxed(),
        sub_entries: other_nodes
            .into_iter()
            .collect::<FuturesUnordered<_>>()
            .boxed(),
        cs_metadata,
        create_bonsai_changeset_hook: None,
        scribe_category: None,
    };
    create_changeset.create(
        CoreContext::test_mock(fb),
        repo,
        MononokeScubaSampleBuilder::with_discard(),
    )
}

pub fn string_to_nodehash(hash: &str) -> HgNodeHash {
    HgNodeHash::from_ascii_str(hash.as_ascii_str().unwrap()).unwrap()
}

pub fn to_mpath(path: RepoPath) -> Result<MPath, Error> {
    let bad_mpath = Error::msg("RepoPath did not convert to MPath");
    path.into_mpath().ok_or(bad_mpath)
}

pub fn to_leaf(
    fut: BoxFuture<'static, Result<(HgFileNodeId, RepoPath), Error>>,
) -> BoxFuture<'static, Result<(Entry<HgManifestId, HgFileNodeId>, RepoPath), Error>> {
    fut.map_ok(|(id, path)| (Entry::Leaf(id), path)).boxed()
}

pub fn to_tree(
    fut: BoxFuture<'static, Result<(HgManifestId, RepoPath), Error>>,
) -> BoxFuture<'static, Result<(Entry<HgManifestId, HgFileNodeId>, RepoPath), Error>> {
    fut.map_ok(|(id, path)| (Entry::Tree(id), path)).boxed()
}
