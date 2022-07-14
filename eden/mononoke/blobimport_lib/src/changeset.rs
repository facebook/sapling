/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use ::manifest::Entry;
use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use failure_ext::Compat;
use failure_ext::FutureFailureErrorExt;
use failure_ext::StreamFailureErrorExt;
use futures::compat::Future01CompatExt;
use futures::compat::Stream01CompatExt;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryFutureExt;
use futures_01_ext::try_boxfuture;
use futures_01_ext::try_boxstream;
use futures_01_ext::BoxFuture;
use futures_01_ext::BoxStream;
use futures_01_ext::FutureExt as _;
use futures_01_ext::StreamExt as _;
use futures_ext::FbTryFutureExt;
use futures_old::future;
use futures_old::future::SharedItem;
use futures_old::stream;
use futures_old::stream::Stream;
use futures_old::Future;
use futures_old::IntoFuture;
use tokio::runtime::Handle;

use blobrepo::BlobRepo;
use blobrepo_hg::create_bonsai_changeset_hook;
use blobrepo_hg::ChangesetHandle;
use blobrepo_hg::CreateChangeset;
use lfs_import_lib::lfs_upload;
use mercurial_revlog::manifest;
use mercurial_revlog::revlog::RevIdx;
use mercurial_revlog::RevlogChangeset;
use mercurial_revlog::RevlogEntry;
use mercurial_revlog::RevlogRepo;
use mercurial_types::blobs::ChangesetMetadata;
use mercurial_types::blobs::ContentBlobMeta;
use mercurial_types::blobs::File;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::blobs::LFSContent;
use mercurial_types::blobs::UploadHgFileContents;
use mercurial_types::blobs::UploadHgFileEntry;
use mercurial_types::blobs::UploadHgNodeHash;
use mercurial_types::blobs::UploadHgTreeEntry;
use mercurial_types::HgBlob;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mercurial_types::MPath;
use mercurial_types::RepoPath;
use mercurial_types::Type;
use mercurial_types::NULL_HASH;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ContentMetadata;
use phases::PhasesArc;
use slog::info;

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
        .with_context(move || format!("While reading changeset {:?}", csid))
        .map_err(Compat)
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
        .with_context(move || format!("While reading root manifest for {:?}", csid))
        .map_err(Compat)
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

                let p1 = parents
                    .next()
                    .unwrap_or_else(|| Ok(None).into_future().boxify());
                let p2 = parents
                    .next()
                    .unwrap_or_else(|| Ok(None).into_future().boxify());

                p1.join(p2)
                    .with_context(move || format!("While reading parents of {:?}", csid))
                    .from_err()
            }
        })
        .join(rootmf.clone().from_err())
        .map(|((p1, p2), rootmf_shared)| match *rootmf_shared {
            None => stream::empty().boxify(),
            Some((_, ref rootmf)) => {
                manifest::new_entry_intersection_stream(rootmf, p1.as_ref(), p2.as_ref())
            }
        })
        .flatten_stream()
        .with_context(move || format!("While reading entries for {:?}", csid))
        .from_err()
        .boxify();

    let revlogcs = revlogcs.map_err(Error::from).boxify();

    let rootmf = rootmf
        .map_err(Error::from)
        .and_then(move |rootmf_shared| match *rootmf_shared {
            None => Ok(None),
            Some((manifest_id, ref mf)) => {
                let mut bytes = Vec::new();
                mf.generate(&mut bytes).with_context(|| {
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
) -> BoxFuture<(Entry<HgManifestId, HgFileNodeId>, RepoPath), Error> {
    let blobrepo = blobrepo.clone();

    let ty = entry.get_type();

    let path = MPath::join_element_opt(path.as_ref(), entry.get_name());
    let path = match path {
        // XXX this shouldn't be possible -- encode this in the type system
        None => {
            return future::err(Error::msg(
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
            let blobstore = blobrepo.get_blobstore().boxed();
            let filestore_config = blobrepo.filestore_config();
            match (ty, is_ext) {
                (Type::Tree, false) => {
                    let upload = UploadHgTreeEntry {
                        upload_node_id,
                        contents: content.into_inner(),
                        p1,
                        p2,
                        path: RepoPath::DirectoryPath(path),
                    };
                    let (_, upload_fut) = try_boxfuture!(upload.upload_as_entry(ctx, blobstore));
                    upload_fut
                }
                (Type::Tree, true) => Err(Error::msg("Inconsistent data: externally stored Tree"))
                    .into_future()
                    .boxify(),
                (Type::File(..), false) => {
                    let upload = UploadHgFileEntry {
                        upload_node_id,
                        contents: UploadHgFileContents::RawBytes(
                            content.into_inner(),
                            filestore_config,
                        ),
                        p1: p1.map(HgFileNodeId::new),
                        p2: p2.map(HgFileNodeId::new),
                    };
                    let upload_fut = upload.upload_as_entry(ctx, blobstore, path);
                    tokio::task::spawn(upload_fut)
                        .flatten_err()
                        .boxed()
                        .compat()
                        .boxify()
                }
                (Type::File(..), true) => {
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
                                p1,
                                p2,
                            };
                            let upload_fut = upload.upload_as_entry(ctx, blobstore, path);
                            tokio::task::spawn(upload_fut)
                                .flatten_err()
                                .boxed()
                                .compat()
                                .boxify()
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
    pub lfs_helper: Option<String>,
    pub concurrent_changesets: usize,
    pub concurrent_blobs: usize,
    pub concurrent_lfs_imports: usize,
    pub fixed_parent_order: HashMap<HgChangesetId, Vec<HgChangesetId>>,
}

impl UploadChangesets {
    pub fn upload(
        self,
        changesets: impl Stream<Item = (RevIdx, HgNodeHash), Error = Error> + Send + 'static,
        is_import_from_beggining: bool,
        origin_repo: Option<BlobRepo>,
    ) -> BoxStream<(RevIdx, (BonsaiChangeset, HgBlobChangeset)), Error> {
        let Self {
            ctx,
            blobrepo,
            revlogrepo,
            lfs_helper,
            concurrent_changesets,
            concurrent_blobs,
            concurrent_lfs_imports,
            fixed_parent_order,
        } = self;

        let mut parent_changeset_handles: HashMap<HgNodeHash, ChangesetHandle> = HashMap::new();

        let handle = try_boxstream!(Handle::try_current().context("No tokio runtime available"));

        let mut scuba_logger = ctx.scuba().clone();
        scuba_logger
            .add("Repo Id", blobrepo.get_repoid().to_string())
            .add("Repo name", blobrepo.name().to_string());

        let lfs_uploader = Arc::new(try_boxstream!(JobProcessor::new(
            {
                cloned!(ctx, blobrepo);
                move |lfs_content| match &lfs_helper {
                    Some(lfs_helper) => {
                        cloned!(ctx, blobrepo, lfs_helper);
                        async move { lfs_upload(&ctx, &blobrepo, &lfs_helper, &lfs_content).await }
                            .boxed()
                            .compat()
                    }
                    .boxify(),
                    None => Err(Error::msg("Cannot blobimport LFS without LFS helper"))
                        .into_future()
                        .boxify(),
                }
            },
            &handle,
            concurrent_lfs_imports,
        )));

        let blob_uploader = Arc::new(try_boxstream!(JobProcessor::new(
            {
                cloned!(ctx, blobrepo, lfs_uploader);
                move |(entry, path)| {
                    upload_entry(ctx.clone(), &blobrepo, lfs_uploader.clone(), entry, path).boxify()
                }
            },
            &handle,
            concurrent_blobs,
        )));

        let create_and_verify_bonsai = create_bonsai_changeset_hook(origin_repo);

        changesets
            .and_then({
                cloned!(ctx, revlogrepo, blobrepo);
                move |(revidx, csid)| {
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
                                        .upload(ctx, blobrepo.get_blobstore().boxed())
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
                        move |(path, entry)| blob_uploader.process((entry, path))
                    });

                    revlogcs
                        .join3(rootmf, entries.collect())
                        .map(move |(cs, rootmf, entries)| (revidx, csid, cs, rootmf, entries))
                }
            })
            .and_then({
                cloned!(ctx);
                move |(revidx, csid, cs, rootmf, entries)| {
                    let parents_from_revlog: Vec<_> =
                        cs.parents().into_iter().map(HgChangesetId::new).collect();

                    if let Some(parent_order) =
                        fixed_parent_order.get(&HgChangesetId::new(csid.clone()))
                    {
                        let actual: HashSet<_> = parents_from_revlog.into_iter().collect();
                        let expected: HashSet<_> = parent_order.iter().copied().collect();
                        if actual != expected {
                            bail!(
                                "Changeset {} has unexpected parents: actual {:?}\nexpected {:?}",
                                csid,
                                actual,
                                expected
                            );
                        }

                        info!(
                            ctx.logger(),
                            "fixing parent order for {}: {:?}", csid, parent_order
                        );
                        Ok((revidx, csid, cs, rootmf, entries, parent_order.clone()))
                    } else {
                        Ok((revidx, csid, cs, rootmf, entries, parents_from_revlog))
                    }
                }
            })
            .map(move |(revidx, csid, cs, rootmf, entries, parents)| {
                let entries = stream::futures_unordered(entries).boxify();

                let (p1handle, p2handle) = {
                    let mut parents = parents.into_iter().map(|p| {
                        let p = p.into_nodehash();
                        let maybe_handle = parent_changeset_handles.get(&p).cloned();

                        if is_import_from_beggining {
                            maybe_handle
                                .unwrap_or_else(|| panic!("parent {} not found for {}", p, csid))
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
                        .unwrap_or_else(|_| panic!("non-utf8 username for {}", csid)),
                    time: cs.time().clone(),
                    extra: cs.extra().clone(),
                    message: String::from_utf8(Vec::from(cs.message()))
                        .unwrap_or_else(|_| panic!("non-utf8 message for {}", csid)),
                };
                let create_changeset = CreateChangeset {
                    expected_nodeid: Some(csid),
                    expected_files: Some(Vec::from(cs.files())),
                    p1: p1handle,
                    p2: p2handle,
                    root_manifest: rootmf.compat().boxed(),
                    sub_entries: entries.compat().boxed(),
                    cs_metadata,
                    create_bonsai_changeset_hook: Some(create_and_verify_bonsai.clone()),
                    scribe_category: None,
                };
                let cshandle =
                    create_changeset.create(ctx.clone(), &blobrepo, scuba_logger.clone());
                parent_changeset_handles.insert(csid, cshandle.clone());

                cloned!(ctx);
                let phases = blobrepo.phases_arc();

                // Uploading changeset and populate phases
                // We know they are public.
                tokio::task::spawn(async move {
                    cshandle
                        .get_completed_changeset()
                        .await
                        .with_context(move || format!("While uploading changeset: {}", csid))
                })
                .flatten_err()
                .boxed()
                .compat()
                .and_then(move |shared| {
                    cloned!(ctx, phases);
                    async move {
                        phases
                            .add_reachable_as_public(&ctx, vec![shared.0.get_changeset_id()])
                            .await?;
                        Ok((revidx, shared))
                    }
                    .boxed()
                    .compat()
                })
                .boxify()
            })
            // This is the number of changesets to upload in parallel. Keep it small to keep the database
            // load under control
            .buffer_unordered(concurrent_changesets)
            .boxify()
    }
}
