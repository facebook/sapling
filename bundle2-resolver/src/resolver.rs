// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use ascii::AsciiString;
use bytes::Bytes;
use futures::{Future, IntoFuture, Stream};
use futures::future::{err, ok};
use futures::stream;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use slog::Logger;

use blobrepo::{BlobEntry, BlobRepo, ChangesetHandle};
use mercurial::changeset::RevlogChangeset;
use mercurial::manifest::revlog::ManifestContent;
use mercurial_bundles::{parts, Bundle2EncodeBuilder, Bundle2Item};
use mercurial_types::{Changeset, HgChangesetId, HgManifestId, MPath, NodeHash, RepoPath};

use changegroup::{convert_to_revlog_changesets, convert_to_revlog_filelog, split_changegroup,
                  Filelog};
use errors::*;
use upload_blobs::{upload_blobs, UploadBlobsType, UploadableBlob};
use wirepackparser::{TreemanifestBundle2Parser, TreemanifestEntry};

type PartId = u32;
type Changesets = Vec<(NodeHash, RevlogChangeset)>;
type Filelogs = HashMap<(NodeHash, RepoPath), <Filelog as UploadableBlob>::Value>;
type Manifests = HashMap<(NodeHash, RepoPath), <TreemanifestEntry as UploadableBlob>::Value>;
type UploadedChangesets = HashMap<NodeHash, ChangesetHandle>;

/// The resolve function takes a bundle2, interprets it's content as Changesets, Filelogs and
/// Manifests and uploades all of them to the provided BlobRepo in the correct order.
/// It returns a Future that contains the response that should be send back to the requester.
pub fn resolve(
    repo: Arc<BlobRepo>,
    logger: Logger,
    heads: Vec<String>,
    bundle2: BoxStream<Bundle2Item, Error>,
) -> BoxFuture<Bytes, Error> {
    info!(logger, "unbundle heads {:?}", heads);

    let resolver = Bundle2Resolver::new(repo, logger);

    let bundle2 = resolver.resolve_start_and_replycaps(bundle2);

    resolver
        .maybe_resolve_changegroup(bundle2)
        .and_then({
            let resolver = resolver.clone();
            move |(cg_push, bundle2)| {
                resolver
                    .maybe_resolve_bookmark_pushkey(bundle2)
                    .map(move |(bookmark_push, bundle2)| (cg_push, bookmark_push, bundle2))
            }
        })
        .and_then(move |(cg_push, bookmark_push, bundle2)| {
            let mut changegroup_id = None;
            let bundle2 = if let Some(cg_push) = cg_push {
                changegroup_id = Some(cg_push.part_id);
                let changesets = cg_push.changesets;
                let filelogs = cg_push.filelogs;

                resolver
                    .resolve_b2xtreegroup2(bundle2)
                    .and_then({
                        let resolver = resolver.clone();

                        move |(manifests, bundle2)| {
                            resolver
                                .upload_changesets(changesets, filelogs, manifests)
                                .map(|()| bundle2)
                        }
                    })
                    .flatten_stream()
                    .boxify()
            } else {
                bundle2
            };

            resolver
                .maybe_resolve_infinitepush_bookmarks(bundle2)
                .and_then({
                    let resolver = resolver.clone();
                    move |(_, bundle2)| resolver.ensure_stream_finished(bundle2)
                })
                .and_then({
                    let resolver = resolver.clone();
                    move |()| match bookmark_push {
                        Some(bookmark_push) => resolver
                            .update_bookmark(&bookmark_push)
                            .map(move |()| Some(bookmark_push.part_id))
                            .boxify(),
                        None => ok(None).boxify(),
                    }
                })
                .and_then(move |bookmark_id| resolver.prepare_response(changegroup_id, bookmark_id))
        })
        .map_err(|err| err.context("bundle2-resolver error").into())
        .boxify()
}

fn next_item(
    bundle2: BoxStream<Bundle2Item, Error>,
) -> BoxFuture<(Option<Bundle2Item>, BoxStream<Bundle2Item, Error>), Error> {
    bundle2.into_future().map_err(|(err, _)| err).boxify()
}

struct ChangegroupPush {
    part_id: PartId,
    changesets: Changesets,
    filelogs: Filelogs,
}

struct BookmarkPush {
    part_id: PartId,
    name: AsciiString,
    old: Option<HgChangesetId>,
    new: Option<HgChangesetId>,
}

/// Holds repo and logger for convienience access from it's methods
#[derive(Clone)]
struct Bundle2Resolver {
    repo: Arc<BlobRepo>,
    logger: Logger,
}

impl Bundle2Resolver {
    fn new(repo: Arc<BlobRepo>, logger: Logger) -> Self {
        Self { repo, logger }
    }

    /// Parse Start and Replycaps and ignore their content
    fn resolve_start_and_replycaps(
        &self,
        bundle2: BoxStream<Bundle2Item, Error>,
    ) -> BoxStream<Bundle2Item, Error> {
        next_item(bundle2)
            .and_then(|(start, bundle2)| match start {
                Some(Bundle2Item::Start(_)) => next_item(bundle2),
                _ => err(format_err!("Expected Bundle2 Start")).boxify(),
            })
            .and_then(|(replycaps, bundle2)| match replycaps {
                Some(Bundle2Item::Replycaps(_, part)) => part.map(|_| bundle2).boxify(),
                _ => err(format_err!("Expected Bundle2 Replycaps")).boxify(),
            })
            .flatten_stream()
            .boxify()
    }

    /// Parse changegroup.
    /// The ChangegroupId will be used in the last step for preparing response
    /// The Changesets should be parsed as RevlogChangesets and used for uploading changesets
    /// The Filelogs should be scheduled for uploading to BlobRepo and the Future resolving in
    /// their upload should be used for uploading changesets
    fn maybe_resolve_changegroup(
        &self,
        bundle2: BoxStream<Bundle2Item, Error>,
    ) -> BoxFuture<(Option<ChangegroupPush>, BoxStream<Bundle2Item, Error>), Error> {
        let repo = self.repo.clone();

        next_item(bundle2)
            .and_then(move |(changegroup, bundle2)| match changegroup {
                Some(Bundle2Item::Changegroup(header, parts))
                | Some(Bundle2Item::B2xInfinitepush(header, parts)) => {
                    let part_id = header.part_id();
                    let (c, f) = split_changegroup(parts);
                    convert_to_revlog_changesets(c)
                        .collect()
                        .join(
                            upload_blobs(
                                repo.clone(),
                                convert_to_revlog_filelog(repo, f),
                                UploadBlobsType::EnsureNoDuplicates,
                            ).map_err(|err| err.context("While uploading File Blobs").into()),
                        )
                        .map(move |(changesets, filelogs)| {
                            let cg_push = ChangegroupPush {
                                part_id,
                                changesets,
                                filelogs,
                            };
                            (Some(cg_push), bundle2)
                        })
                        .boxify()
                }
                Some(part) => ok((None, stream::once(Ok(part)).chain(bundle2).boxify())).boxify(),
                _ => err(format_err!("Unexpected Bundle2 stream end")).boxify(),
            })
            .map_err(|err| err.context("While resolving Changegroup").into())
            .boxify()
    }

    /// Parses bookmark pushkey part if it exists
    /// Errors if `namespace` is not "bookmarks"
    fn maybe_resolve_bookmark_pushkey(
        &self,
        bundle2: BoxStream<Bundle2Item, Error>,
    ) -> BoxFuture<(Option<BookmarkPush>, BoxStream<Bundle2Item, Error>), Error> {
        next_item(bundle2)
            .and_then(move |(newpart, bundle2)| match newpart {
                Some(Bundle2Item::Pushkey(header, emptypart)) => {
                    let part_id = header.part_id();
                    let mparams = header.mparams();
                    try_boxfuture!(
                        mparams
                            .get("namespace")
                            .ok_or(format_err!("pushkey: `namespace` parameter is not set"))
                            .and_then(|namespace| if namespace != "bookmarks".as_bytes() {
                                Err(format_err!(
                                    "pushkey: unexpected namespace: {:?}",
                                    namespace
                                ))
                            } else {
                                Ok(())
                            })
                    );

                    let name = try_boxfuture!(get_ascii_param(mparams, "key"));
                    let old = try_boxfuture!(get_optional_changeset_param(mparams, "old"));
                    let new = try_boxfuture!(get_optional_changeset_param(mparams, "new"));

                    let bookmark_push = BookmarkPush {
                        part_id,
                        name,
                        old,
                        new,
                    };
                    emptypart
                        .map(move |_| (Some(bookmark_push), bundle2.boxify()))
                        .boxify()
                }
                Some(part) => ok((None, stream::once(Ok(part)).chain(bundle2).boxify())).boxify(),
                None => ok((None, bundle2.boxify())).boxify(),
            })
            .map_err(|err| err.context("While resolving Pushkey").into())
            .boxify()
    }

    fn update_bookmark(&self, bookmark_push: &BookmarkPush) -> BoxFuture<(), Error> {
        let mut txn = self.repo.update_bookmark_transaction();

        match (bookmark_push.new, bookmark_push.old) {
            (Some(new), Some(old)) => {
                try_boxfuture!(txn.update(&bookmark_push.name, &new, &old));
            }
            (Some(new), None) => {
                try_boxfuture!(txn.create(&bookmark_push.name, &new));
            }
            (None, Some(old)) => {
                try_boxfuture!(txn.delete(&bookmark_push.name, &old));
            }
            _ => {
                return ok(()).boxify();
            }
        }

        txn.commit()
    }

    /// Parse b2xtreegroup2.
    /// The Manifests should be scheduled for uploading to BlobRepo and the Future resolving in
    /// their upload as well as their parsed content should be used for uploading changesets.
    fn resolve_b2xtreegroup2(
        &self,
        bundle2: BoxStream<Bundle2Item, Error>,
    ) -> BoxFuture<(Manifests, BoxStream<Bundle2Item, Error>), Error> {
        let repo = self.repo.clone();

        next_item(bundle2)
            .and_then(move |(b2xtreegroup2, bundle2)| match b2xtreegroup2 {
                Some(Bundle2Item::B2xTreegroup2(_, parts)) => {
                    upload_blobs(
                        repo,
                        TreemanifestBundle2Parser::new(parts),
                        UploadBlobsType::IgnoreDuplicates,
                    ).map_err(|err| err.context("While uploading Manifest Blobs").into())
                        .map(move |manifests| (manifests, bundle2))
                        .boxify()
                }
                _ => err(format_err!("Expected Bundle2 B2xTreegroup2")).boxify(),
            })
            .map_err(|err| err.context("While resolving B2xTreegroup2").into())
            .boxify()
    }

    /// Parse b2xinfinitepushscratchbookmarks.
    /// This part is ignored, so just parse it and forget it
    fn maybe_resolve_infinitepush_bookmarks(
        &self,
        bundle2: BoxStream<Bundle2Item, Error>,
    ) -> BoxFuture<((), BoxStream<Bundle2Item, Error>), Error> {
        next_item(bundle2)
            .and_then(
                move |(infinitepushbookmarks, bundle2)| match infinitepushbookmarks {
                    Some(Bundle2Item::B2xInfinitepushBookmarks(_, bookmarks)) => {
                        bookmarks.collect().map(|_| ((), bundle2)).boxify()
                    }
                    None => Ok(((), bundle2)).into_future().boxify(),
                    _ => err(format_err!(
                        "Expected B2xInfinitepushBookmarks or end of the stream"
                    )).boxify(),
                },
            )
            .map_err(|err| {
                err.context("While resolving B2xInfinitepushBookmarks")
                    .into()
            })
            .boxify()
    }

    /// Takes parsed Changesets and scheduled for upload Filelogs and Manifests. The content of
    /// Manifests is used to figure out DAG of dependencies between a given Changeset and the
    /// Manifests and Filelogs it adds.
    /// The Changesets are scheduled for uploading and a Future is returned, whose completion means
    /// that the changesets were uploaded
    fn upload_changesets(
        &self,
        changesets: Changesets,
        filelogs: Filelogs,
        manifests: Manifests,
    ) -> BoxFuture<(), Error> {
        fn upload_changeset(
            repo: Arc<BlobRepo>,
            node: NodeHash,
            revlog_cs: RevlogChangeset,
            mut uploaded_changesets: UploadedChangesets,
            filelogs: &Filelogs,
            manifests: &Manifests,
        ) -> BoxFuture<UploadedChangesets, Error> {
            let (p1, p2) = {
                let (p1, p2) = revlog_cs.parents().get_nodes();
                (
                    get_parent(&repo, &uploaded_changesets, p1.cloned()),
                    get_parent(&repo, &uploaded_changesets, p2.cloned()),
                )
            };
            let (root_manifest, entries) = try_boxfuture!(walk_manifests(
                *revlog_cs.manifestid(),
                &manifests,
                &filelogs
            ));

            p1.join(p2)
                .and_then(move |(p1, p2)| {
                    let scheduled_uploading = repo.create_changeset(
                        p1,
                        p2,
                        root_manifest,
                        entries,
                        String::from_utf8(revlog_cs.user().into())?,
                        revlog_cs.time().clone(),
                        revlog_cs.extra().clone(),
                        String::from_utf8(revlog_cs.comments().into())?,
                    );

                    uploaded_changesets.insert(node, scheduled_uploading);
                    Ok(uploaded_changesets)
                })
                .boxify()
        }

        let repo = self.repo.clone();

        debug!(self.logger, "changesets: {:?}", changesets);
        debug!(self.logger, "filelogs: {:?}", filelogs.keys());
        debug!(self.logger, "manifests: {:?}", manifests.keys());

        stream::iter_ok(changesets)
            .fold(
                HashMap::new(),
                move |uploaded_changesets, (node, revlog_cs)| {
                    upload_changeset(
                        repo.clone(),
                        node.clone(),
                        revlog_cs,
                        uploaded_changesets,
                        &filelogs,
                        &manifests,
                    ).map_err(move |err| {
                        err.context(format!(
                            "While trying to upload Changeset with id {:?}",
                            node
                        ))
                    })
                },
            )
            .and_then(|uploaded_changesets| {
                stream::futures_unordered(
                    uploaded_changesets
                        .into_iter()
                        .map(|(_, cs)| cs.get_completed_changeset()),
                ).map_err(Error::from)
                    .for_each(|_| Ok(()))
            })
            .map_err(|err| err.context("While uploading Changesets to BlobRepo").into())
            .boxify()
    }

    /// Ensures that the next item in stream is None
    fn ensure_stream_finished(
        &self,
        bundle2: BoxStream<Bundle2Item, Error>,
    ) -> BoxFuture<(), Error> {
        next_item(bundle2)
            .and_then(|(none, _)| {
                ensure_msg!(none.is_none(), "Expected end of Bundle2");
                Ok(())
            })
            .boxify()
    }

    /// Takes a changegroup id and prepares a Bytes response containing Bundle2 with reply to
    /// changegroup part saying that the push was successful
    fn prepare_response(
        &self,
        changegroup_id: Option<PartId>,
        bookmark_id: Option<PartId>,
    ) -> BoxFuture<Bytes, Error> {
        let writer = Cursor::new(Vec::new());
        let mut bundle = Bundle2EncodeBuilder::new(writer);
        // Mercurial currently hangs while trying to read compressed bundles over the wire:
        // https://bz.mercurial-scm.org/show_bug.cgi?id=5646
        // TODO: possibly enable compression support once this is fixed.
        bundle.set_compressor_type(None);
        if let Some(changegroup_id) = changegroup_id {
            bundle.add_part(try_boxfuture!(parts::replychangegroup_part(
                parts::ChangegroupApplyResult::Success { heads_num_diff: 0 },
                changegroup_id,
            )));
        }
        if let Some(part_id) = bookmark_id {
            bundle.add_part(try_boxfuture!(parts::replypushkey_part(true, part_id)));
        }
        bundle
            .build()
            .map(|cursor| Bytes::from(cursor.into_inner()))
            .map_err(|err| err.context("While preparing response").into())
            .boxify()
    }
}

/// Retrieves the parent from uploaded changesets, if it is missing then fetches it from BlobRepo
fn get_parent(
    repo: &BlobRepo,
    map: &UploadedChangesets,
    p: Option<NodeHash>,
) -> BoxFuture<Option<ChangesetHandle>, Error> {
    match p {
        None => ok(None).boxify(),
        Some(p) => match map.get(&p) {
            None => repo.get_changeset_by_changesetid(&HgChangesetId::new(p))
                .map(|cs| Some(cs.into()))
                .boxify(),
            Some(cs) => ok(Some(cs.clone())).boxify(),
        },
    }
}

type BlobFuture = BoxFuture<(BlobEntry, RepoPath), Error>;
type BlobStream = BoxStream<(BlobEntry, RepoPath), Error>;

/// In order to generate the DAG of dependencies between Root Manifest and other Manifests and
/// Filelogs we need to walk that DAG.
/// This function starts with the Root Manifest Id and returns Future for Root Manifest and Stream
/// of all dependent Manifests and Filelogs that were provided in this push.
fn walk_manifests(
    manifest_root_id: HgManifestId,
    manifests: &Manifests,
    filelogs: &Filelogs,
) -> Result<(BlobFuture, BlobStream)> {
    fn walk_helper(
        path_taken: &RepoPath,
        manifest_content: &ManifestContent,
        manifests: &Manifests,
        filelogs: &Filelogs,
    ) -> Result<Vec<BlobFuture>> {
        if path_taken.len() > 4096 {
            bail_msg!(
                "Exceeded max manifest path during walking with path: {:?}",
                path_taken
            );
        }

        let mut entries: Vec<BlobFuture> = Vec::new();
        for (name, details) in manifest_content.files.iter() {
            let nodehash = details.entryid().clone().into_nodehash();
            let next_path = MPath::join_opt(path_taken.mpath(), name);
            let next_path = match next_path {
                Some(path) => path,
                None => bail_msg!("internal error: joined root path with root manifest"),
            };

            if details.is_tree() {
                let key = (nodehash, RepoPath::DirectoryPath(next_path));

                if let Some(&(ref manifest_content, ref blobfuture)) = manifests.get(&key) {
                    entries.push(
                        blobfuture
                            .clone()
                            .map(|it| (*it).clone())
                            .from_err()
                            .boxify(),
                    );
                    entries.append(&mut walk_helper(
                        &key.1,
                        manifest_content,
                        manifests,
                        filelogs,
                    )?);
                }
            } else {
                if let Some(blobfuture) = filelogs.get(&(nodehash, RepoPath::FilePath(next_path))) {
                    entries.push(
                        blobfuture
                            .clone()
                            .map(|it| (*it).clone())
                            .from_err()
                            .boxify(),
                    );
                }
            }
        }

        Ok(entries)
    }

    let &(ref manifest_content, ref manifest_root) = manifests
        .get(&(manifest_root_id.clone().into_nodehash(), RepoPath::root()))
        .ok_or_else(|| format_err!("Missing root tree manifest"))?;

    Ok((
        manifest_root
            .clone()
            .map(|it| (*it).clone())
            .from_err()
            .boxify(),
        stream::futures_unordered(walk_helper(
            &RepoPath::root(),
            &manifest_content,
            manifests,
            filelogs,
        )?).map_err(move |err| {
            err.context(format!(
                "While walking dependencies of Root Manifest with id {:?}",
                manifest_root_id
            )).into()
        })
            .boxify(),
    ))
}

fn get_ascii_param(params: &HashMap<String, Bytes>, param: &str) -> Result<AsciiString> {
    let val = params
        .get(param)
        .ok_or(format_err!("`{}` parameter is not set", param))?;
    AsciiString::from_ascii(val.to_vec())
        .map_err(|err| format_err!("`{}` parameter is not ascii: {}", param, err))
}

fn get_optional_changeset_param(
    params: &HashMap<String, Bytes>,
    param: &str,
) -> Result<Option<HgChangesetId>> {
    let val = get_ascii_param(params, param)?;

    if val.is_empty() {
        Ok(None)
    } else {
        Ok(Some(HgChangesetId::from_ascii_str(&val)?))
    }
}
