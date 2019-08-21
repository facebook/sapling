// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{
    err_msg, Error, Fail, FutureFailureErrorExt, FutureFailureExt, ResultExt, StreamFailureErrorExt,
};
use futures::{
    future::{self, SharedItem},
    stream::{self, Stream},
    sync::oneshot,
    Future, IntoFuture,
};
use futures_ext::{
    spawn_future, try_boxfuture, try_boxstream, BoxFuture, BoxStream, FutureExt, StreamExt,
};
use scuba_ext::ScubaSampleBuilder;
use tokio::executor::DefaultExecutor;
use tracing::{trace_args, EventId, Traced};

use blobrepo::{
    BlobRepo, ChangesetHandle, ChangesetMetadata, ContentBlobMeta, CreateChangeset,
    HgBlobChangeset, HgBlobEntry, UploadHgFileContents, UploadHgFileEntry, UploadHgNodeHash,
    UploadHgTreeEntry,
};
use lfs_import_lib::lfs_upload;
use mercurial_revlog::{
    file::{File, LFSContent},
    manifest, RevlogChangeset, RevlogEntry, RevlogRepo,
};
use mercurial_types::{
    HgBlob, HgChangesetId, HgFileNodeId, HgManifestId, HgNodeHash, MPath, RepoPath, Type, NULL_HASH,
};
use mononoke_types::{BonsaiChangeset, ContentMetadata};
use phases::Phases;

use crate::concurrency::JobProcessor;

struct ParseChangeset {
    revlogcs: BoxFuture<SharedItem<RevlogChangeset>, Error>,
    rootmf:
        BoxFuture<Option<(HgManifestId, HgBlob, Option<HgNodeHash>, Option<HgNodeHash>)>, Error>,
    entries: BoxStream<(Option<MPath>, RevlogEntry), Error>,
}

// Extracts all the data from revlog repo that commit API may need.
fn parse_changeset(revlog_repo: RevlogRepo, csid: HgChangesetId) -> ParseChangeset {
    let revlogcs = revlog_repo
        .get_changeset(csid)
        .with_context(move |_| format!("While reading changeset {:?}", csid))
        .map_err(Fail::compat)
        .boxify()
        .shared();

    let rootmf = revlogcs
        .clone()
        .map_err(Error::from)
        .and_then({
            let revlog_repo = revlog_repo.clone();
            move |cs| {
                if cs.manifestid().into_nodehash() == NULL_HASH {
                    future::ok(None).boxify()
                } else {
                    revlog_repo
                        .get_root_manifest(cs.manifestid())
                        .map({
                            let manifest_id = cs.manifestid();
                            move |rootmf| Some((manifest_id, rootmf))
                        })
                        .boxify()
                }
            }
        })
        .with_context(move |_| format!("While reading root manifest for {:?}", csid))
        .map_err(Fail::compat)
        .boxify()
        .shared();

    let entries = revlogcs
        .clone()
        .map_err(Error::from)
        .and_then({
            let revlog_repo = revlog_repo.clone();
            move |cs| {
                let mut parents = cs
                    .parents()
                    .into_iter()
                    .map(HgChangesetId::new)
                    .map(|csid| {
                        let revlog_repo = revlog_repo.clone();
                        revlog_repo
                            .get_changeset(csid)
                            .and_then(move |cs| {
                                if cs.manifestid().into_nodehash() == NULL_HASH {
                                    future::ok(None).boxify()
                                } else {
                                    revlog_repo
                                        .get_root_manifest(cs.manifestid())
                                        .map(Some)
                                        .boxify()
                                }
                            })
                            .boxify()
                    });

                let p1 = parents.next().unwrap_or(Ok(None).into_future().boxify());
                let p2 = parents.next().unwrap_or(Ok(None).into_future().boxify());

                p1.join(p2)
                    .with_context(move |_| format!("While reading parents of {:?}", csid))
                    .from_err()
            }
        })
        .join(rootmf.clone().from_err())
        .map(|((p1, p2), rootmf_shared)| match *rootmf_shared {
            None => stream::empty().boxify(),
            Some((_, ref rootmf)) => {
                manifest::new_entry_intersection_stream(&rootmf, p1.as_ref(), p2.as_ref())
            }
        })
        .flatten_stream()
        .with_context(move |_| format!("While reading entries for {:?}", csid))
        .from_err()
        .boxify();

    let revlogcs = revlogcs.map_err(Error::from).boxify();

    let rootmf = rootmf
        .map_err(Error::from)
        .and_then(move |rootmf_shared| match *rootmf_shared {
            None => Ok(None),
            Some((manifest_id, ref mf)) => {
                let mut bytes = Vec::new();
                mf.generate(&mut bytes).with_context(|_| {
                    format!("While generating root manifest blob for {:?}", csid)
                })?;

                let (p1, p2) = mf.parents().get_nodes();
                Ok(Some((
                    manifest_id,
                    HgBlob::from(Bytes::from(bytes)),
                    p1,
                    p2,
                )))
            }
        })
        .boxify();

    ParseChangeset {
        revlogcs,
        rootmf,
        entries,
    }
}

fn upload_entry(
    ctx: CoreContext,
    blobrepo: &BlobRepo,
    lfs_uploader: Arc<JobProcessor<LFSContent, ContentMetadata>>,
    entry: RevlogEntry,
    path: Option<MPath>,
) -> BoxFuture<(HgBlobEntry, RepoPath), Error> {
    let blobrepo = blobrepo.clone();

    let ty = entry.get_type();

    let path = MPath::join_element_opt(path.as_ref(), entry.get_name());
    let path = match path {
        // XXX this shouldn't be possible -- encode this in the type system
        None => {
            return future::err(err_msg(
                "internal error: joined root path with root manifest",
            ))
            .boxify();
        }
        Some(path) => path,
    };

    let content = entry.get_raw_content();
    let is_ext = entry.is_ext();
    let parents = entry.get_parents();

    (content, is_ext, parents)
        .into_future()
        .and_then(move |(content, is_ext, parents)| {
            let (p1, p2) = parents.get_nodes();
            let upload_node_id = UploadHgNodeHash::Checked(entry.get_hash().into_nodehash());
            match (ty, is_ext) {
                (Type::Tree, false) => {
                    let upload = UploadHgTreeEntry {
                        upload_node_id,
                        contents: content.into_inner(),
                        p1,
                        p2,
                        path: RepoPath::DirectoryPath(path),
                    };
                    let (_, upload_fut) = try_boxfuture!(upload.upload(ctx, &blobrepo));
                    upload_fut
                }
                (Type::Tree, true) => Err(err_msg("Inconsistent data: externally stored Tree"))
                    .into_future()
                    .boxify(),
                (Type::File(ft), false) => {
                    let upload = UploadHgFileEntry {
                        upload_node_id,
                        contents: UploadHgFileContents::RawBytes(content.into_inner()),
                        file_type: ft,
                        p1: p1.map(HgFileNodeId::new),
                        p2: p2.map(HgFileNodeId::new),
                        path,
                    };
                    let (_, upload_fut) = try_boxfuture!(upload.upload(ctx, &blobrepo));
                    spawn_future(upload_fut).boxify()
                }
                (Type::File(ft), true) => {
                    let p1 = p1.map(HgFileNodeId::new);
                    let p2 = p2.map(HgFileNodeId::new);

                    let file = File::new(content, p1.clone(), p2.clone());
                    let lfs_content = try_boxfuture!(file.get_lfs_content());

                    lfs_uploader
                        .process(lfs_content.clone())
                        .and_then(move |meta| {
                            let cbmeta = ContentBlobMeta {
                                id: meta.content_id,
                                size: meta.total_size,
                                copy_from: lfs_content.copy_from(),
                            };

                            let upload = UploadHgFileEntry {
                                upload_node_id,
                                contents: UploadHgFileContents::ContentUploaded(cbmeta),
                                file_type: ft,
                                p1,
                                p2,
                                path,
                            };
                            let (_, upload_fut) = try_boxfuture!(upload.upload(ctx, &blobrepo));
                            spawn_future(upload_fut).boxify()
                        })
                        .boxify()
                }
            }
        })
        .boxify()
}

pub struct UploadChangesets {
    pub ctx: CoreContext,
    pub blobrepo: BlobRepo,
    pub revlogrepo: RevlogRepo,
    pub changeset: Option<HgNodeHash>,
    pub skip: Option<usize>,
    pub commits_limit: Option<usize>,
    pub phases_store: Arc<dyn Phases>,
    pub lfs_helper: Option<String>,
    pub concurrent_changesets: usize,
    pub concurrent_blobs: usize,
    pub concurrent_lfs_imports: usize,
}

impl UploadChangesets {
    pub fn upload(self) -> BoxStream<SharedItem<(BonsaiChangeset, HgBlobChangeset)>, Error> {
        let Self {
            ctx,
            blobrepo,
            revlogrepo,
            changeset,
            skip,
            commits_limit,
            phases_store,
            lfs_helper,
            concurrent_changesets,
            concurrent_blobs,
            concurrent_lfs_imports,
        } = self;

        let changesets = match changeset {
            Some(hash) => future::ok(hash).into_stream().boxify(),
            None => revlogrepo.changesets().boxify(),
        };

        let changesets = match skip {
            None => changesets,
            Some(skip) => changesets.skip(skip as u64).boxify(),
        };

        let changesets = match commits_limit {
            None => changesets,
            Some(limit) => changesets.take(limit as u64).boxify(),
        };

        let is_import_from_beggining = changeset.is_none() && skip.is_none();
        let mut parent_changeset_handles: HashMap<HgNodeHash, ChangesetHandle> = HashMap::new();

        let mut executor = DefaultExecutor::current();

        let event_id = EventId::new();

        let lfs_uploader = Arc::new(try_boxstream!(JobProcessor::new(
            {
                cloned!(ctx, blobrepo);
                move |lfs_content| match &lfs_helper {
                    Some(lfs_helper) => lfs_upload(
                        ctx.clone(),
                        blobrepo.clone(),
                        lfs_helper.clone(),
                        lfs_content,
                    )
                    .boxify(),
                    None => Err(err_msg("Cannot blobimport LFS without LFS helper"))
                        .into_future()
                        .boxify(),
                }
            },
            &mut executor,
            concurrent_lfs_imports,
        )));

        let blob_uploader = Arc::new(try_boxstream!(JobProcessor::new(
            {
                cloned!(ctx, blobrepo, lfs_uploader);
                move |(entry, path)| {
                    upload_entry(ctx.clone(), &blobrepo, lfs_uploader.clone(), entry, path).boxify()
                }
            },
            &mut executor,
            concurrent_blobs,
        )));

        changesets
            .and_then({
                cloned!(ctx, revlogrepo, blobrepo);
                move |csid| {
                    let ParseChangeset {
                        revlogcs,
                        rootmf,
                        entries,
                    } = parse_changeset(revlogrepo.clone(), HgChangesetId::new(csid));

                    let rootmf = rootmf.map({
                        cloned!(ctx, blobrepo);
                        move |rootmf| {
                            match rootmf {
                                None => future::ok(None).boxify(),
                                Some((manifest_id, blob, p1, p2)) => {
                                    let upload = UploadHgTreeEntry {
                                        // The root tree manifest is expected to have the wrong hash in
                                        // hybrid mode. This will probably never go away for
                                        // compatibility with old repositories.
                                        upload_node_id: UploadHgNodeHash::Supplied(
                                            manifest_id.into_nodehash(),
                                        ),
                                        contents: blob.into_inner(),
                                        p1,
                                        p2,
                                        path: RepoPath::root(),
                                    };
                                    upload
                                        .upload(ctx, &blobrepo)
                                        .into_future()
                                        .and_then(|(_, entry)| entry)
                                        .map(Some)
                                        .boxify()
                                }
                            }
                        }
                    });

                    let entries = entries.map({
                        cloned!(blob_uploader);
                        move |(path, entry)|  {
                            blob_uploader.process((entry, path))
                        }
                    });

                    revlogcs
                        .join3(rootmf, entries.collect())
                        .map(move |(cs, rootmf, entries)| (csid, cs, rootmf, entries))
                        .traced_with_id(&ctx.trace(), "parse changeset from revlog", trace_args!(), event_id)
                }
            })
            .map(move |(csid, cs, rootmf, entries)| {
                let entries = stream::futures_unordered(entries).boxify();

                let (p1handle, p2handle) = {
                    let mut parents = cs.parents().into_iter().map(|p| {
                        let maybe_handle = parent_changeset_handles.get(&p).cloned();

                        if is_import_from_beggining {
                            maybe_handle.expect(&format!("parent {} not found for {}", p, csid))
                        } else {
                            let hg_cs_id = HgChangesetId::new(p);

                            maybe_handle.unwrap_or_else({
                                cloned!(ctx, blobrepo);
                                move || ChangesetHandle::ready_cs_handle(ctx, blobrepo, hg_cs_id)
                            })
                        }
                    });

                    (parents.next(), parents.next())
                };

                let cs_metadata = ChangesetMetadata {
                    user: String::from_utf8(Vec::from(cs.user()))
                        .expect(&format!("non-utf8 username for {}", csid)),
                    time: cs.time().clone(),
                    extra: cs.extra().clone(),
                    comments: String::from_utf8(Vec::from(cs.comments()))
                        .expect(&format!("non-utf8 comments for {}", csid)),
                };
                let create_changeset = CreateChangeset {
                    expected_nodeid: Some(csid),
                    expected_files: Some(Vec::from(cs.files())),
                    p1: p1handle,
                    p2: p2handle,
                    root_manifest: rootmf,
                    sub_entries: entries,
                    cs_metadata,
                    // Repositories can contain case conflicts - we still need to import them
                    must_check_case_conflicts: false,
                    // Blobimported commits are always public
                    draft: false,
                };
                let cshandle =
                    create_changeset.create(ctx.clone(), &blobrepo, ScubaSampleBuilder::with_discard());
                parent_changeset_handles.insert(csid, cshandle.clone());

                cloned!(ctx, blobrepo, phases_store);

                // Uploading changeset and populate phases
                // We know they are public.
                oneshot::spawn(cshandle
                    .get_completed_changeset()
                    .with_context(move |_| format!("While uploading changeset: {}", csid))
                    .from_err(), &executor)
                    .and_then(move |shared| phases_store.add_reachable_as_public(ctx, blobrepo, vec![shared.0.get_changeset_id()]).map(move |_| shared))
                    .boxify()
            })
            // This is the number of changesets to upload in parallel. Keep it small to keep the database
            // load under control
            .buffer_unordered(concurrent_changesets)
            .boxify()
    }
}
