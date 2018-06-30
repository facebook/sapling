// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::io::Cursor;
use std::ops::AddAssign;
use std::sync::Arc;

use ascii::AsciiString;
use blobrepo::{BlobRepo, ChangesetHandle, ContentBlobInfo, CreateChangeset, HgBlobEntry};
use bookmarks;
use bytes::Bytes;
use failure::{Compat, FutureFailureErrorExt, StreamFailureErrorExt};
use futures::{Future, IntoFuture, Stream};
use futures::future::{self, err, ok, Shared};
use futures::stream;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use mercurial::changeset::RevlogChangeset;
use mercurial::manifest::{Details, ManifestContent};
use mercurial_bundles::{parts, Bundle2EncodeBuilder, Bundle2Item};
use mercurial_types::{HgChangesetId, HgManifestId, HgNodeHash, HgNodeKey, MPath, RepoPath,
                      NULL_HASH};
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use slog::Logger;
use stats::*;

use changegroup::{convert_to_revlog_changesets, convert_to_revlog_filelog, split_changegroup};
use errors::*;
use upload_blobs::{upload_hg_blobs, UploadBlobsType, UploadableHgBlob};
use wirepackparser::{TreemanifestBundle2Parser, TreemanifestEntry};

type PartId = u32;
type Changesets = Vec<(HgNodeHash, RevlogChangeset)>;
type Filelogs = HashMap<HgNodeKey, Shared<BoxFuture<(HgBlobEntry, RepoPath), Compat<Error>>>>;
type ContentBlobs = HashMap<HgNodeKey, ContentBlobInfo>;
type Manifests = HashMap<HgNodeKey, <TreemanifestEntry as UploadableHgBlob>::Value>;
type UploadedChangesets = HashMap<HgNodeHash, ChangesetHandle>;

/// The resolve function takes a bundle2, interprets it's content as Changesets, Filelogs and
/// Manifests and uploades all of them to the provided BlobRepo in the correct order.
/// It returns a Future that contains the response that should be send back to the requester.
pub fn resolve(
    repo: Arc<BlobRepo>,
    logger: Logger,
    scuba_logger: ScubaSampleBuilder,
    heads: Vec<String>,
    bundle2: BoxStream<Bundle2Item, Error>,
) -> BoxFuture<Bytes, Error> {
    info!(logger, "unbundle heads {:?}", heads);

    let resolver = Bundle2Resolver::new(repo, logger, scuba_logger);

    let bundle2 = resolver.resolve_start_and_replycaps(bundle2);

    resolver
        .maybe_resolve_changegroup(bundle2)
        .and_then({
            let resolver = resolver.clone();
            move |(cg_push, bundle2)| {
                resolver
                    .resolve_multiple_parts(bundle2, Bundle2Resolver::maybe_resolve_pushkey)
                    .map(move |(pushkeys, bundle2)| {
                        let bookmark_push: Vec<_> = pushkeys
                            .into_iter()
                            .filter_map(|pushkey| match pushkey {
                                Pushkey::Phases => None,
                                Pushkey::BookmarkPush(bp) => Some(bp),
                            })
                            .collect();

                        STATS::bookmark_pushkeys_count.add_value(bookmark_push.len() as i64);

                        (cg_push, bookmark_push, bundle2)
                    })
            }
        })
        .and_then({
            let resolver = resolver.clone();
            move |(cg_push, bookmark_push, bundle2)| {
                if let Some(cg_push) = cg_push {
                    resolver
                        .resolve_b2xtreegroup2(bundle2)
                        .map(|(manifests, bundle2)| {
                            (Some((cg_push, manifests)), bookmark_push, bundle2)
                        })
                        .boxify()
                } else {
                    ok((None, bookmark_push, bundle2)).boxify()
                }
            }
        })
        .and_then({
            let resolver = resolver.clone();
            move |(cg_and_manifests, bookmark_push, bundle2)| {
                if let Some((cg_push, manifests)) = cg_and_manifests {
                    let changegroup_id = Some(cg_push.part_id);
                    resolver
                        .upload_changesets(cg_push, manifests)
                        .map(move |()| (changegroup_id, bookmark_push, bundle2))
                        .boxify()
                } else {
                    ok((None, bookmark_push, bundle2)).boxify()
                }
            }
        })
        .and_then({
            let resolver = resolver.clone();
            move |(changegroup_id, bookmark_push, bundle2)| {
                resolver
                    .maybe_resolve_infinitepush_bookmarks(bundle2)
                    .map(move |((), bundle2)| (changegroup_id, bookmark_push, bundle2))
            }
        })
        .and_then({
            let resolver = resolver.clone();
            move |(changegroup_id, bookmark_push, bundle2)| {
                resolver
                    .ensure_stream_finished(bundle2)
                    .map(move |()| (changegroup_id, bookmark_push))
            }
        })
        .and_then({
            let resolver = resolver.clone();
            move |(changegroup_id, bookmark_push)| {
                (move || {
                    let bookmark_ids: Vec<_> = bookmark_push.iter().map(|bp| bp.part_id).collect();

                    let mut txn = resolver.repo.update_bookmark_transaction();
                    for bp in bookmark_push {
                        try_boxfuture!(add_bookmark_to_transaction(&mut txn, bp));
                    }
                    txn.commit()
                        .and_then(|ok| {
                            if ok {
                                Ok(())
                            } else {
                                Err(format_err!("Bookmark transaction failed"))
                            }
                        })
                        .map(move |()| (changegroup_id, bookmark_ids))
                        .boxify()
                })()
                    .context("While updating Bookmarks")
                    .from_err()
            }
        })
        .and_then(move |(changegroup_id, bookmark_ids)| {
            resolver.prepare_response(changegroup_id, bookmark_ids)
        })
        .context("bundle2-resolver error")
        .from_err()
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
    content_blobs: ContentBlobs,
}

enum Pushkey {
    BookmarkPush(BookmarkPush),
    Phases,
}

struct BookmarkPush {
    part_id: PartId,
    name: bookmarks::Bookmark,
    old: Option<HgChangesetId>,
    new: Option<HgChangesetId>,
}

/// Holds repo and logger for convienience access from it's methods
#[derive(Clone)]
struct Bundle2Resolver {
    repo: Arc<BlobRepo>,
    logger: Logger,
    scuba_logger: ScubaSampleBuilder,
}

impl Bundle2Resolver {
    fn new(repo: Arc<BlobRepo>, logger: Logger, scuba_logger: ScubaSampleBuilder) -> Self {
        Self {
            repo,
            logger,
            scuba_logger,
        }
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
                        .and_then(|changesets| {
                            upload_hg_blobs(
                                repo.clone(),
                                convert_to_revlog_filelog(repo, f),
                                UploadBlobsType::EnsureNoDuplicates,
                            ).map(move |upload_map| {
                                let mut filelogs = HashMap::new();
                                let mut content_blobs = HashMap::new();
                                for (node_key, (cbinfo, file_upload)) in upload_map {
                                    filelogs.insert(node_key.clone(), file_upload);
                                    content_blobs.insert(node_key, cbinfo);
                                }
                                (changesets, filelogs, content_blobs)
                            })
                                .context("While uploading File Blobs")
                                .from_err()
                        })
                        .map(move |(changesets, filelogs, content_blobs)| {
                            let cg_push = ChangegroupPush {
                                part_id,
                                changesets,
                                filelogs,
                                content_blobs,
                            };
                            (Some(cg_push), bundle2)
                        })
                        .boxify()
                }
                Some(part) => ok((None, stream::once(Ok(part)).chain(bundle2).boxify())).boxify(),
                _ => err(format_err!("Unexpected Bundle2 stream end")).boxify(),
            })
            .context("While resolving Changegroup")
            .from_err()
            .boxify()
    }

    /// Parses pushkey part if it exists
    /// Returns an error if the pushkey namespace is unknown
    fn maybe_resolve_pushkey(
        &self,
        bundle2: BoxStream<Bundle2Item, Error>,
    ) -> BoxFuture<(Option<Pushkey>, BoxStream<Bundle2Item, Error>), Error> {
        next_item(bundle2)
            .and_then(move |(newpart, bundle2)| match newpart {
                Some(Bundle2Item::Pushkey(header, emptypart)) => {
                    let namespace = try_boxfuture!(
                        header
                            .mparams()
                            .get("namespace")
                            .ok_or(format_err!("pushkey: `namespace` parameter is not set"))
                    );

                    let pushkey = match &namespace[..] {
                        b"phases" => Pushkey::Phases,
                        b"bookmarks" => {
                            let part_id = header.part_id();
                            let mparams = header.mparams();
                            let name = try_boxfuture!(get_ascii_param(mparams, "key"));
                            let name = bookmarks::Bookmark::new_ascii(name);
                            let old = try_boxfuture!(get_optional_changeset_param(mparams, "old"));
                            let new = try_boxfuture!(get_optional_changeset_param(mparams, "new"));

                            Pushkey::BookmarkPush(BookmarkPush {
                                part_id,
                                name,
                                old,
                                new,
                            })
                        }
                        _ => {
                            return err(format_err!(
                                "pushkey: unexpected namespace: {:?}",
                                namespace
                            )).boxify()
                        }
                    };

                    emptypart.map(move |_| (Some(pushkey), bundle2)).boxify()
                }
                Some(part) => ok((None, stream::once(Ok(part)).chain(bundle2).boxify())).boxify(),
                None => ok((None, bundle2)).boxify(),
            })
            .context("While resolving Pushkey")
            .from_err()
            .boxify()
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
                    upload_hg_blobs(
                        repo,
                        TreemanifestBundle2Parser::new(parts),
                        UploadBlobsType::IgnoreDuplicates,
                    ).context("While uploading Manifest Blobs")
                        .from_err()
                        .map(move |manifests| (manifests, bundle2))
                        .boxify()
                }
                _ => err(format_err!("Expected Bundle2 B2xTreegroup2")).boxify(),
            })
            .context("While resolving B2xTreegroup2")
            .from_err()
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
            .context("While resolving B2xInfinitepushBookmarks")
            .from_err()
            .boxify()
    }

    /// Takes parsed Changesets and scheduled for upload Filelogs and Manifests. The content of
    /// Manifests is used to figure out DAG of dependencies between a given Changeset and the
    /// Manifests and Filelogs it adds.
    /// The Changesets are scheduled for uploading and a Future is returned, whose completion means
    /// that the changesets were uploaded
    fn upload_changesets(
        &self,
        cg_push: ChangegroupPush,
        manifests: Manifests,
    ) -> BoxFuture<(), Error> {
        let changesets = cg_push.changesets;
        let filelogs = cg_push.filelogs;
        let content_blobs = cg_push.content_blobs;

        self.scuba_logger
            .clone()
            .add("changeset_count", changesets.len())
            .add("manifests_count", manifests.len())
            .add("filelogs_count", filelogs.len())
            .log_with_msg("Size of unbundle", None);

        STATS::changesets_count.add_value(changesets.len() as i64);
        STATS::manifests_count.add_value(manifests.len() as i64);
        STATS::filelogs_count.add_value(filelogs.len() as i64);
        STATS::content_blobs_count.add_value(content_blobs.len() as i64);

        fn upload_changeset(
            repo: Arc<BlobRepo>,
            scuba_logger: ScubaSampleBuilder,
            node: HgNodeHash,
            revlog_cs: RevlogChangeset,
            mut uploaded_changesets: UploadedChangesets,
            filelogs: &Filelogs,
            manifests: &Manifests,
            content_blobs: &ContentBlobs,
        ) -> BoxFuture<UploadedChangesets, Error> {
            let (p1, p2) = {
                (
                    get_parent(&repo, &uploaded_changesets, revlog_cs.p1),
                    get_parent(&repo, &uploaded_changesets, revlog_cs.p2),
                )
            };
            let NewBlobs {
                root_manifest,
                sub_entries,
                // XXX use these content blobs in the future
                content_blobs: _content_blobs,
            } = try_boxfuture!(NewBlobs::new(
                *revlog_cs.manifestid(),
                &manifests,
                &filelogs,
                &content_blobs,
            ));

            p1.join(p2)
                .with_context(move |_| format!("While fetching parents for Changeset {}", node))
                .from_err()
                .and_then(move |(p1, p2)| {
                    let create_changeset = CreateChangeset {
                        expected_nodeid: Some(node),
                        expected_files: Some(Vec::from(revlog_cs.files())),
                        p1,
                        p2,
                        root_manifest,
                        sub_entries,
                        // XXX pass content blobs to CreateChangeset here
                        user: String::from_utf8(revlog_cs.user().into())?,
                        time: revlog_cs.time().clone(),
                        extra: revlog_cs.extra().clone(),
                        comments: String::from_utf8(revlog_cs.comments().into())?,
                    };
                    let scheduled_uploading = create_changeset.create(&repo, scuba_logger);

                    uploaded_changesets.insert(node, scheduled_uploading);
                    Ok(uploaded_changesets)
                })
                .boxify()
        }

        let repo = self.repo.clone();

        let changesets_hashes: Vec<_> = changesets.iter().map(|(hash, _)| *hash).collect();

        trace!(self.logger, "changesets: {:?}", changesets);
        trace!(self.logger, "filelogs: {:?}", filelogs.keys());
        trace!(self.logger, "manifests: {:?}", manifests.keys());
        trace!(self.logger, "content blobs: {:?}", content_blobs.keys());

        let scuba_logger = self.scuba_logger.clone();
        stream::iter_ok(changesets)
            .fold(
                HashMap::new(),
                move |uploaded_changesets, (node, revlog_cs)| {
                    upload_changeset(
                        repo.clone(),
                        scuba_logger.clone(),
                        node.clone(),
                        revlog_cs,
                        uploaded_changesets,
                        &filelogs,
                        &manifests,
                        &content_blobs,
                    )
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
            .context(ErrorKind::WhileUploadingData(changesets_hashes))
            .from_err()
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
        bookmark_ids: Vec<PartId>,
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
        for part_id in bookmark_ids {
            bundle.add_part(try_boxfuture!(parts::replypushkey_part(true, part_id)));
        }
        bundle
            .build()
            .map(|cursor| Bytes::from(cursor.into_inner()))
            .context("While preparing response")
            .from_err()
            .boxify()
    }

    /// A method that can use any of the above maybe_resolve_* methods to return
    /// a Vec of (potentailly multiple) Part rather than an Option of Part.
    /// The original use case is to parse multiple pushkey Parts since bundle2 gets
    /// one pushkey part per bookmark.
    fn resolve_multiple_parts<T, Func>(
        &self,
        bundle2: BoxStream<Bundle2Item, Error>,
        mut maybe_resolve: Func,
    ) -> BoxFuture<(Vec<T>, BoxStream<Bundle2Item, Error>), Error>
    where
        Func: FnMut(&Self, BoxStream<Bundle2Item, Error>)
            -> BoxFuture<(Option<T>, BoxStream<Bundle2Item, Error>), Error>
            + Send
            + 'static,
        T: Send + 'static,
    {
        let this = self.clone();
        future::loop_fn((Vec::new(), bundle2), move |(mut result, bundle2)| {
            maybe_resolve(&this, bundle2).map(move |(maybe_element, bundle2)| match maybe_element {
                None => future::Loop::Break((result, bundle2)),
                Some(element) => {
                    result.push(element);
                    future::Loop::Continue((result, bundle2))
                }
            })
        }).boxify()
    }
}

fn add_bookmark_to_transaction(
    txn: &mut Box<bookmarks::Transaction>,
    bookmark_push: BookmarkPush,
) -> Result<()> {
    match (bookmark_push.new, bookmark_push.old) {
        (Some(new), Some(old)) => txn.update(&bookmark_push.name, &new, &old),
        (Some(new), None) => txn.create(&bookmark_push.name, &new),
        (None, Some(old)) => txn.delete(&bookmark_push.name, &old),
        _ => Ok(()),
    }
}

/// Retrieves the parent from uploaded changesets, if it is missing then fetches it from BlobRepo
fn get_parent(
    repo: &BlobRepo,
    map: &UploadedChangesets,
    p: Option<HgNodeHash>,
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

type HgBlobFuture = BoxFuture<(HgBlobEntry, RepoPath), Error>;
type HgBlobStream = BoxStream<(HgBlobEntry, RepoPath), Error>;

/// In order to generate the DAG of dependencies between Root Manifest and other Manifests and
/// Filelogs we need to walk that DAG.
/// This represents the manifests and file nodes introduced by a particular changeset.
struct NewBlobs {
    // root_manifest can be None f.e. when commit removes all the content of the repo
    root_manifest: BoxFuture<Option<(HgBlobEntry, RepoPath)>, Error>,
    // sub_entries has both submanifest and filenode entries.
    sub_entries: HgBlobStream,
    // This is returned as a Vec rather than a Stream so that the path and metadata are
    // available before the content blob is uploaded. This will allow creating and uploading
    // changeset blobs without being blocked on content blob uploading being complete.
    content_blobs: Vec<ContentBlobInfo>,
}

struct WalkHelperCounters {
    manifests_count: usize,
    filelogs_count: usize,
    content_blobs_count: usize,
}

impl AddAssign for WalkHelperCounters {
    fn add_assign(&mut self, other: WalkHelperCounters) {
        *self = Self {
            manifests_count: self.manifests_count + other.manifests_count,
            filelogs_count: self.filelogs_count + other.filelogs_count,
            content_blobs_count: self.content_blobs_count + other.content_blobs_count,
        };
    }
}

impl NewBlobs {
    fn new(
        manifest_root_id: HgManifestId,
        manifests: &Manifests,
        filelogs: &Filelogs,
        content_blobs: &ContentBlobs,
    ) -> Result<Self> {
        if manifest_root_id.into_nodehash() == NULL_HASH {
            // If manifest root id is NULL_HASH then there is no content in this changest
            return Ok(Self {
                root_manifest: ok(None).boxify(),
                sub_entries: stream::empty().boxify(),
                content_blobs: Vec::new(),
            });
        }

        let root_key = HgNodeKey {
            path: RepoPath::root(),
            hash: manifest_root_id.clone().into_nodehash(),
        };

        let &(ref manifest_content, ref p1, ref p2, ref manifest_root) = manifests
            .get(&root_key)
            .ok_or_else(|| format_err!("Missing root tree manifest"))?;

        let (entries, content_blobs, counters) = Self::walk_helper(
            &RepoPath::root(),
            &manifest_content,
            get_manifest_parent_content(manifests, RepoPath::root(), p1.clone()),
            get_manifest_parent_content(manifests, RepoPath::root(), p2.clone()),
            manifests,
            filelogs,
            content_blobs,
        )?;

        STATS::per_changeset_manifests_count.add_value(counters.manifests_count as i64);
        STATS::per_changeset_filelogs_count.add_value(counters.filelogs_count as i64);
        STATS::per_changeset_content_blobs_count.add_value(counters.content_blobs_count as i64);

        Ok(Self {
            root_manifest: manifest_root
                .clone()
                .map(|it| Some((*it).clone()))
                .from_err()
                .boxify(),
            sub_entries: stream::futures_unordered(entries)
                .with_context(move |_| {
                    format!(
                        "While walking dependencies of Root Manifest with id {:?}",
                        manifest_root_id
                    )
                })
                .from_err()
                .boxify(),
            content_blobs,
        })
    }

    fn walk_helper(
        path_taken: &RepoPath,
        manifest_content: &ManifestContent,
        p1: Option<&ManifestContent>,
        p2: Option<&ManifestContent>,
        manifests: &Manifests,
        filelogs: &Filelogs,
        content_blobs: &ContentBlobs,
    ) -> Result<(Vec<HgBlobFuture>, Vec<ContentBlobInfo>, WalkHelperCounters)> {
        if path_taken.len() > 4096 {
            bail_msg!(
                "Exceeded max manifest path during walking with path: {:?}",
                path_taken
            );
        }

        let mut entries: Vec<HgBlobFuture> = Vec::new();
        let mut cbinfos: Vec<ContentBlobInfo> = Vec::new();
        let mut counters = WalkHelperCounters {
            manifests_count: 0,
            filelogs_count: 0,
            content_blobs_count: 0,
        };

        for (name, details) in manifest_content.files.iter() {
            if is_entry_present_in_parent(p1, name, details)
                || is_entry_present_in_parent(p2, name, details)
            {
                // If one of the parents contains exactly the same version of entry then either that
                // file or manifest subtree is not new
                continue;
            }

            let nodehash = details.entryid().clone().into_nodehash();
            let next_path = MPath::join_opt(path_taken.mpath(), name);
            let next_path = match next_path {
                Some(path) => path,
                None => bail_msg!("internal error: joined root path with root manifest"),
            };

            if details.is_tree() {
                let key = HgNodeKey {
                    path: RepoPath::DirectoryPath(next_path),
                    hash: nodehash,
                };

                if let Some(&(ref manifest_content, ref p1, ref p2, ref blobfuture)) =
                    manifests.get(&key)
                {
                    counters.manifests_count += 1;
                    entries.push(
                        blobfuture
                            .clone()
                            .map(|it| (*it).clone())
                            .from_err()
                            .boxify(),
                    );
                    let (mut walked_entries, mut walked_cbinfos, sub_counters) =
                        Self::walk_helper(
                            &key.path,
                            manifest_content,
                            get_manifest_parent_content(manifests, key.path.clone(), p1.clone()),
                            get_manifest_parent_content(manifests, key.path.clone(), p2.clone()),
                            manifests,
                            filelogs,
                            content_blobs,
                        )?;
                    entries.append(&mut walked_entries);
                    cbinfos.append(&mut walked_cbinfos);
                    counters += sub_counters;
                }
            } else {
                let key = HgNodeKey {
                    path: RepoPath::FilePath(next_path),
                    hash: nodehash,
                };
                if let Some(blobfuture) = filelogs.get(&key) {
                    counters.filelogs_count += 1;
                    counters.content_blobs_count += 1;
                    entries.push(
                        blobfuture
                            .clone()
                            .map(|it| (*it).clone())
                            .from_err()
                            .boxify(),
                    );
                    match content_blobs.get(&key) {
                        Some(cbinfo) => cbinfos.push(cbinfo.clone()),
                        None => {
                            bail_msg!("internal error: content blob future missing for filenode")
                        }
                    }
                }
            }
        }

        Ok((entries, cbinfos, counters))
    }
}

fn get_manifest_parent_content(
    manifests: &Manifests,
    path: RepoPath,
    p: Option<HgNodeHash>,
) -> Option<&ManifestContent> {
    p.and_then(|p| manifests.get(&HgNodeKey { path, hash: p }))
        .map(|&(ref content, ..)| content)
}

fn is_entry_present_in_parent(
    p: Option<&ManifestContent>,
    name: &MPath,
    details: &Details,
) -> bool {
    match p.and_then(|p| p.files.get(name)) {
        None => false,
        Some(parent_details) => parent_details == details,
    }
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
