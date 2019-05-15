// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::changegroup::{
    convert_to_revlog_changesets, convert_to_revlog_filelog, split_changegroup,
};
use crate::errors::*;
use crate::getbundle_response;
use crate::stats::*;
use crate::upload_blobs::{upload_hg_blobs, UploadBlobsType, UploadableHgBlob};
use ascii::AsciiString;
use blobrepo::{
    BlobRepo, ChangesetHandle, ChangesetMetadata, ContentBlobInfo, CreateChangeset, HgBlobEntry,
};
use bookmarks::{Bookmark, BookmarkUpdateReason, BundleReplayData, Transaction};
use bytes::{Bytes, BytesMut};
use cloned::cloned;
use context::CoreContext;
use failure::{err_msg, format_err, Compat};
use failure_ext::{bail_msg, ensure_msg, FutureFailureErrorExt, StreamFailureErrorExt};
use futures::future::{self, err, ok, Shared};
use futures::stream;
use futures::{Future, IntoFuture, Stream};
use futures_ext::{try_boxfuture, BoxFuture, BoxStream, FutureExt, StreamExt};
use futures_stats::Timed;
use hooks::{ChangesetHookExecutionID, FileHookExecutionID, HookExecution, HookManager};
use mercurial::changeset::RevlogChangeset;
use mercurial::manifest::{Details, ManifestContent};
use mercurial_bundles::{
    create_bundle_stream, parts, Bundle2EncodeBuilder, Bundle2Item, PartHeaderType,
};
use mercurial_types::{
    HgChangesetId, HgManifestId, HgNodeHash, HgNodeKey, MPath, RepoPath, NULL_HASH,
};
use metaconfig_types::{BookmarkAttrs, PushrebaseParams, RepoReadOnly};
use mononoke_types::{BlobstoreValue, ChangesetId, RawBundle2, RawBundle2Id};
use phases::{self, Phases};
use pushrebase;
use reachabilityindex::LeastCommonAncestorsHint;
use scribe_commit_queue::{self, ScribeCommitQueue};
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use slog::{debug, trace};
use std::collections::HashMap;
use std::io::Cursor;
use std::ops::AddAssign;
use std::sync::{Arc, Mutex};
use wirepack::{TreemanifestBundle2Parser, TreemanifestEntry};

type PartId = u32;
type Changesets = Vec<(HgChangesetId, RevlogChangeset)>;
type Filelogs = HashMap<HgNodeKey, Shared<BoxFuture<(HgBlobEntry, RepoPath), Compat<Error>>>>;
type ContentBlobs = HashMap<HgNodeKey, ContentBlobInfo>;
type Manifests = HashMap<HgNodeKey, <TreemanifestEntry as UploadableHgBlob>::Value>;
type UploadedChangesets = HashMap<HgChangesetId, ChangesetHandle>;

/// The resolve function takes a bundle2, interprets it's content as Changesets, Filelogs and
/// Manifests and uploades all of them to the provided BlobRepo in the correct order.
/// It returns a Future that contains the response that should be send back to the requester.
pub fn resolve(
    ctx: CoreContext,
    repo: BlobRepo,
    pushrebase: PushrebaseParams,
    bookmark_attrs: BookmarkAttrs,
    _heads: Vec<String>,
    bundle2: BoxStream<Bundle2Item, Error>,
    hook_manager: Arc<HookManager>,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    phases_hint: Arc<dyn Phases>,
    readonly: RepoReadOnly,
    maybe_full_content: Option<Arc<Mutex<Bytes>>>,
) -> BoxFuture<Bytes, Error> {
    let resolver =
        Bundle2Resolver::new(ctx.clone(), repo, pushrebase, bookmark_attrs, hook_manager);
    let bundle2 = resolver.resolve_start_and_replycaps(bundle2);

    resolver
        .maybe_resolve_commonheads(bundle2)
        .and_then({
            cloned!(resolver);
            move |(maybe_commonheads, bundle2)| {
                resolver.maybe_resolve_pushvars(bundle2).and_then(
                    move |(maybe_pushvars, bundle2)| {
                        let mut bypass_readonly = false;
                        // check the bypass condition
                        if let Some(ref pushvars) = maybe_pushvars {
                            bypass_readonly = pushvars
                                .get("BYPASS_READONLY")
                                .map(|s| s.to_ascii_lowercase())
                                == Some("true".into());
                        }
                        // force the readonly check
                        match (readonly, bypass_readonly) {
                            (RepoReadOnly::ReadOnly(reason), false) => {
                                future::err(ErrorKind::RepoReadOnly(reason).into()).left_future()
                            }
                            _ => future::ok((maybe_pushvars, maybe_commonheads, bundle2))
                                .right_future(),
                        }
                    },
                )
            }
        })
        .and_then({
            cloned!(resolver);
            move |(maybe_pushvars, maybe_commonheads, bundle2)| {
                resolver
                    .is_next_part_pushkey(bundle2)
                    .map(move |(pushkey_next, bundle2)| {
                        (maybe_pushvars, maybe_commonheads, pushkey_next, bundle2)
                    })
            }
        })
        .and_then(
            move |(maybe_pushvars, maybe_commonheads, pushkey_next, bundle2)| {
                let mut allow_non_fast_forward = false;
                // check the bypass condition
                if let Some(ref pushvars) = maybe_pushvars {
                    allow_non_fast_forward = pushvars
                        .get("NON_FAST_FORWARD")
                        .map(|s| s.to_ascii_lowercase())
                        == Some("true".into());
                }

                if let Some(commonheads) = maybe_commonheads {
                    if pushkey_next {
                        resolve_bookmark_only_pushrebase(
                            ctx,
                            resolver,
                            bundle2,
                            allow_non_fast_forward,
                            maybe_full_content,
                            lca_hint,
                        )
                    } else {
                        resolve_pushrebase(
                            ctx,
                            commonheads,
                            resolver,
                            bundle2,
                            maybe_pushvars,
                            lca_hint,
                            phases_hint,
                            maybe_full_content,
                        )
                    }
                } else {
                    resolve_push(
                        ctx,
                        resolver,
                        bundle2,
                        allow_non_fast_forward,
                        maybe_full_content,
                        lca_hint,
                    )
                }
            },
        )
        .boxify()
}

fn resolve_push(
    ctx: CoreContext,
    resolver: Bundle2Resolver,
    bundle2: BoxStream<Bundle2Item, Error>,
    allow_non_fast_forward: bool,
    maybe_full_content: Option<Arc<Mutex<Bytes>>>,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
) -> BoxFuture<Bytes, Error> {
    resolver
        .maybe_resolve_changegroup(ctx.clone(), bundle2)
        .and_then({
            cloned!(resolver);
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
            cloned!(ctx, resolver);
            move |(cg_push, bookmark_push, bundle2)| {
                if let Some(cg_push) = cg_push {
                    resolver
                        .resolve_b2xtreegroup2(ctx, bundle2)
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
            cloned!(ctx, resolver);
            move |(cg_and_manifests, bookmark_push, bundle2)| {
                if let Some((cg_push, manifests)) = cg_and_manifests {
                    let changegroup_id = Some(cg_push.part_id);
                    resolver
                        .upload_changesets(ctx, cg_push, manifests)
                        .map(move |()| (changegroup_id, bookmark_push, bundle2))
                        .boxify()
                } else {
                    ok((None, bookmark_push, bundle2)).boxify()
                }
            }
        })
        .and_then({
            cloned!(resolver);
            move |(changegroup_id, bookmark_push, bundle2)| {
                resolver
                    .maybe_resolve_infinitepush_bookmarks(bundle2)
                    .map(move |((), bundle2)| (changegroup_id, bookmark_push, bundle2))
            }
        })
        .and_then({
            cloned!(resolver);
            move |(changegroup_id, bookmark_push, bundle2)| {
                resolver
                    .ensure_stream_finished(bundle2, maybe_full_content)
                    .map(move |maybe_raw_bundle2_id| {
                        (changegroup_id, bookmark_push, maybe_raw_bundle2_id)
                    })
            }
        })
        .and_then({
            cloned!(resolver);
            move |(changegroup_id, bookmark_push, maybe_raw_bundle2_id)| {
                (move || {
                    let bookmark_ids: Vec<_> = bookmark_push.iter().map(|bp| bp.part_id).collect();
                    let reason = BookmarkUpdateReason::Push {
                        bundle_replay_data: maybe_raw_bundle2_id
                            .map(|id| BundleReplayData::new(id)),
                    };
                    resolver
                        .resolve_bookmark_pushes(
                            bookmark_push,
                            reason,
                            lca_hint,
                            allow_non_fast_forward,
                        )
                        .map(move |()| (changegroup_id, bookmark_ids))
                        .boxify()
                })()
                .context("While updating Bookmarks")
                .from_err()
            }
        })
        .and_then(move |(changegroup_id, bookmark_ids)| {
            resolver.prepare_push_response(changegroup_id, bookmark_ids)
        })
        .context("bundle2_resolver error")
        .from_err()
        .boxify()
}

fn resolve_pushrebase(
    ctx: CoreContext,
    commonheads: CommonHeads,
    resolver: Bundle2Resolver,
    bundle2: BoxStream<Bundle2Item, Error>,
    maybe_pushvars: Option<HashMap<String, Bytes>>,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    phases_hint: Arc<dyn Phases>,
    maybe_full_content: Option<Arc<Mutex<Bytes>>>,
) -> BoxFuture<Bytes, Error> {
    resolver
        .resolve_b2xtreegroup2(ctx.clone(), bundle2)
        .and_then({
            cloned!(ctx, resolver);
            move |(manifests, bundle2)| {
                resolver
                    .maybe_resolve_changegroup(ctx, bundle2)
                    .map(move |(cg_push, bundle2)| (cg_push, manifests, bundle2))
            }
        })
        .and_then(|(cg_push, manifests, bundle2)| {
            cg_push
                .ok_or(err_msg("Empty pushrebase"))
                .into_future()
                .map(move |cg_push| (cg_push, manifests, bundle2))
        })
        .and_then(
            |(cg_push, manifests, bundle2)| match cg_push.mparams.get("onto").cloned() {
                Some(onto_bookmark) => {
                    let v = Vec::from(onto_bookmark.as_ref());
                    let onto_bookmark = String::from_utf8(v)?;
                    let onto_bookmark = Bookmark::new(onto_bookmark)?;
                    let onto_bookmark = pushrebase::OntoBookmarkParams {
                        bookmark: onto_bookmark,
                    };
                    Ok((onto_bookmark, cg_push, manifests, bundle2))
                }
                None => Err(err_msg("onto is not specified")),
            },
        )
        .and_then({
            cloned!(ctx, resolver);
            move |(onto_params, cg_push, manifests, bundle2)| {
                let changesets = cg_push.changesets.clone();
                resolver
                    .upload_changesets(ctx, cg_push, manifests)
                    .map(move |()| (changesets, onto_params, bundle2))
            }
        })
        .and_then({
            cloned!(resolver);
            move |(changesets, onto_params, bundle2)| {
                resolver
                    .resolve_multiple_parts(bundle2, Bundle2Resolver::maybe_resolve_pushkey)
                    .and_then({
                        cloned!(resolver);
                        move |(pushkeys, bundle2)| {
                            let bookmark_pushes: Vec<_> = pushkeys
                                .into_iter()
                                .filter_map(|pushkey| match pushkey {
                                    Pushkey::Phases => None,
                                    Pushkey::BookmarkPush(bp) => Some(bp),
                                })
                                .collect();

                            if bookmark_pushes.len() > 1 {
                                return future::err(format_err!(
                                    "Too many pushkey parts: {:?}",
                                    bookmark_pushes
                                ))
                                .boxify();
                            }

                            let bookmark_push_part_id = match bookmark_pushes.get(0) {
                                Some(bk_push) if bk_push.name != onto_params.bookmark => {
                                    return future::err(format_err!(
                                        "allowed only pushes of {} bookmark: {:?}",
                                        onto_params.bookmark,
                                        bookmark_pushes
                                    ))
                                    .boxify();
                                }
                                Some(bk_push) => Some(bk_push.part_id),
                                None => None,
                            };

                            resolver
                                .ensure_stream_finished(bundle2, maybe_full_content)
                                .map(move |maybe_raw_bundle2_id| {
                                    (
                                        changesets,
                                        bookmark_push_part_id,
                                        onto_params,
                                        maybe_raw_bundle2_id,
                                    )
                                })
                                .boxify()
                        }
                    })
            }
        })
        .and_then({
            cloned!(ctx, resolver);
            move |(changesets, bookmark_push_part_id, onto_params, maybe_raw_bundle2_id)| {
                resolver
                    .run_hooks(
                        ctx.clone(),
                        changesets.clone(),
                        maybe_pushvars,
                        &onto_params.bookmark,
                    )
                    .map_err(|err| match err {
                        RunHooksError::Failures((cs_hook_failures, file_hook_failures)) => {
                            let mut err_msgs = vec![];
                            for (exec_id, exec_info) in cs_hook_failures {
                                if let HookExecution::Rejected(info) = exec_info {
                                    err_msgs.push(format!(
                                        "{} for {}: {}",
                                        exec_id.hook_name, exec_id.cs_id, info.description
                                    ));
                                }
                            }
                            for (exec_id, exec_info) in file_hook_failures {
                                if let HookExecution::Rejected(info) = exec_info {
                                    err_msgs.push(format!(
                                        "{} for {}: {}",
                                        exec_id.hook_name, exec_id.cs_id, info.description
                                    ));
                                }
                            }
                            err_msg(format!("hooks failed:\n{}", err_msgs.join("\n")))
                        }
                        RunHooksError::Error(err) => err,
                    })
                    .and_then(move |()| {
                        resolver
                            .pushrebase(ctx, changesets.clone(), &onto_params, maybe_raw_bundle2_id)
                            .map(move |pushrebased_rev| {
                                (pushrebased_rev, onto_params, bookmark_push_part_id)
                            })
                    })
            }
        })
        .and_then(
            move |(
                (pushrebased_rev, pushrebased_changesets),
                onto_params,
                bookmark_push_part_id,
            )| {
                // TODO: (dbudischek) T41565649 log pushed changesets as well, not only pushrebased
                let new_commits = pushrebased_changesets.iter().map(|p| p.id_new).collect();

                resolver
                    .log_commits_to_scribe(ctx.clone(), new_commits)
                    .and_then(move |_| {
                        resolver.prepare_pushrebase_response(
                            ctx,
                            commonheads,
                            pushrebased_rev,
                            pushrebased_changesets,
                            onto_params.bookmark,
                            lca_hint,
                            phases_hint,
                            bookmark_push_part_id,
                        )
                    })
            },
        )
        .boxify()
}

/// Do the right thing when pushrebase-enabled client only wants to manipulate bookmarks
fn resolve_bookmark_only_pushrebase(
    ctx: CoreContext,
    resolver: Bundle2Resolver,
    bundle2: BoxStream<Bundle2Item, Error>,
    allow_non_fast_forward: bool,
    maybe_full_content: Option<Arc<Mutex<Bytes>>>,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
) -> BoxFuture<Bytes, Error> {
    // TODO: we probably run hooks even if no changesets are pushed?
    //       however, current run_hooks implementation will no-op such thing
    resolver
        .resolve_multiple_parts(bundle2, Bundle2Resolver::maybe_resolve_pushkey)
        .and_then({
            cloned!(resolver);
            move |(pushkeys, bundle2)| {
                let pushkeys_len = pushkeys.len();
                let bookmark_pushes: Vec<_> = pushkeys
                    .into_iter()
                    .filter_map(|pushkey| match pushkey {
                        Pushkey::Phases => None,
                        Pushkey::BookmarkPush(bp) => Some(bp),
                    })
                    .collect();

                // this means we filtered some Phase pushkeys out
                // which is not expected
                if bookmark_pushes.len() != pushkeys_len {
                    return err(err_msg("Expected bookmark-only push, phases pushkey found"))
                        .boxify();
                }

                if bookmark_pushes.len() != 1 {
                    return future::err(format_err!(
                        "Too many pushkey parts: {:?}",
                        bookmark_pushes
                    ))
                    .boxify();
                }
                let bookmark_push = bookmark_pushes.into_iter().nth(0).unwrap();
                resolver
                    .ensure_stream_finished(bundle2, maybe_full_content)
                    .map(move |maybe_raw_bundle2_id| (bookmark_push, maybe_raw_bundle2_id))
                    .boxify()
            }
        })
        .and_then({
            cloned!(resolver);
            move |(bookmark_push, maybe_raw_bundle2_id)| {
                let part_id = bookmark_push.part_id;
                let pushes = vec![bookmark_push];
                let reason = BookmarkUpdateReason::Pushrebase {
                    // Since this a bookmark-only pushrebase, there are no changeset timestamps
                    bundle_replay_data: maybe_raw_bundle2_id.map(|id| BundleReplayData::new(id)),
                };
                resolver
                    .resolve_bookmark_pushes(pushes, reason, lca_hint, allow_non_fast_forward)
                    .and_then(move |_| ok(part_id).boxify())
            }
        })
        .and_then({
            cloned!(resolver, ctx);
            move |bookmark_push_part_id| {
                resolver.prepare_push_bookmark_response(ctx, bookmark_push_part_id, true)
            }
        })
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
    mparams: HashMap<String, Bytes>,
    draft: bool,
}

struct CommonHeads {
    heads: Vec<HgChangesetId>,
}

enum Pushkey {
    BookmarkPush(BookmarkPush),
    Phases,
}

#[derive(Debug)]
struct BookmarkPush {
    part_id: PartId,
    name: Bookmark,
    old: Option<HgChangesetId>,
    new: Option<HgChangesetId>,
}

struct BonsaiBookmarkPush {
    name: Bookmark,
    old: Option<ChangesetId>,
    new: Option<ChangesetId>,
}

impl BonsaiBookmarkPush {
    fn new(
        ctx: CoreContext,
        repo: &BlobRepo,
        bookmark_push: BookmarkPush,
    ) -> impl Future<Item = BonsaiBookmarkPush, Error = Error> + Send {
        fn bonsai_from_hg_opt(
            ctx: CoreContext,
            repo: &BlobRepo,
            cs_id: Option<HgChangesetId>,
        ) -> impl Future<Item = Option<ChangesetId>, Error = Error> {
            match cs_id {
                None => future::ok(None).left_future(),
                Some(cs_id) => repo.get_bonsai_from_hg(ctx, cs_id).right_future(),
            }
        }

        let BookmarkPush {
            part_id: _,
            name,
            old,
            new,
        } = bookmark_push;

        (
            bonsai_from_hg_opt(ctx.clone(), repo, old),
            bonsai_from_hg_opt(ctx, repo, new),
        )
            .into_future()
            .map(move |(old, new)| BonsaiBookmarkPush { name, old, new })
    }
}

/// Holds repo and logger for convienience access from it's methods
#[derive(Clone)]
struct Bundle2Resolver {
    ctx: CoreContext,
    repo: BlobRepo,
    pushrebase: PushrebaseParams,
    bookmark_attrs: BookmarkAttrs,
    hook_manager: Arc<HookManager>,
    scribe_commit_queue: Arc<ScribeCommitQueue>,
}

impl Bundle2Resolver {
    fn new(
        ctx: CoreContext,
        repo: BlobRepo,
        pushrebase: PushrebaseParams,
        bookmark_attrs: BookmarkAttrs,
        hook_manager: Arc<HookManager>,
    ) -> Self {
        let scribe_commit_queue = match pushrebase.commit_scribe_category.clone() {
            Some(category) => Arc::new(scribe_commit_queue::LogToScribe::new_with_default_scribe(
                category,
            )),
            None => Arc::new(scribe_commit_queue::LogToScribe::new_with_discard()),
        };

        Self {
            ctx,
            repo,
            pushrebase,
            bookmark_attrs,
            hook_manager,
            scribe_commit_queue,
        }
    }

    /// Produce a future that creates a transaction with potentitally multiple bookmark pushes
    fn resolve_bookmark_pushes(
        &self,
        bookmark_pushes: Vec<BookmarkPush>,
        reason: BookmarkUpdateReason,
        lca_hint: Arc<dyn LeastCommonAncestorsHint>,
        allow_non_fast_forward: bool,
    ) -> impl Future<Item = (), Error = Error> {
        let resolver = self.clone();
        let ctx = resolver.ctx.clone();
        let repo = resolver.repo.clone();
        let bookmark_attrs = resolver.bookmark_attrs.clone();

        let bookmarks_push_fut = bookmark_pushes
            .into_iter()
            .map(move |bp| {
                BonsaiBookmarkPush::new(ctx.clone(), &repo, bp).and_then({
                    cloned!(repo, ctx, lca_hint, bookmark_attrs);
                    move |bp| {
                        check_bookmark_push_allowed(
                            ctx.clone(),
                            repo.clone(),
                            bookmark_attrs,
                            allow_non_fast_forward,
                            bp,
                            lca_hint,
                        )
                    }
                })
            })
            .collect::<Vec<_>>();

        future::join_all(bookmarks_push_fut).and_then({
            cloned!(resolver);
            move |bonsai_bookmark_pushes| {
                let mut txn = resolver
                    .repo
                    .update_bookmark_transaction(resolver.ctx.clone());
                for bp in bonsai_bookmark_pushes {
                    try_boxfuture!(add_bookmark_to_transaction(&mut txn, bp, reason.clone(),));
                }
                txn.commit()
                    .and_then(|ok| {
                        if ok {
                            Ok(())
                        } else {
                            Err(format_err!("Bookmark transaction failed"))
                        }
                    })
                    .boxify()
            }
        })
    }

    /// Peek at the next `bundle2` item and check if it is a `Pushkey` part
    /// Return unchanged `bundle2`
    fn is_next_part_pushkey(
        &self,
        bundle2: BoxStream<Bundle2Item, Error>,
    ) -> BoxFuture<(bool, BoxStream<Bundle2Item, Error>), Error> {
        next_item(bundle2)
            .and_then(|(start, bundle2)| match start {
                Some(part) => {
                    if let Bundle2Item::Pushkey(header, box_future) = part {
                        ok((
                            true,
                            stream::once(Ok(Bundle2Item::Pushkey(header, box_future)))
                                .chain(bundle2)
                                .boxify(),
                        ))
                        .boxify()
                    } else {
                        ok((false, stream::once(Ok(part)).chain(bundle2).boxify())).boxify()
                    }
                }
                _ => ok((false, bundle2)).boxify(),
            })
            .boxify()
    }

    /// Preserve the full raw content of the bundle2 for later replay
    fn maybe_save_full_content_bundle2(
        &self,
        maybe_full_content: Option<Arc<Mutex<Bytes>>>,
    ) -> BoxFuture<Option<RawBundle2Id>, Error> {
        match maybe_full_content {
            Some(full_content) => {
                let blob = RawBundle2::new_bytes(full_content.lock().unwrap().clone()).into_blob();
                let ctx = self.ctx.clone();
                self.repo
                    .upload_blob_no_alias(ctx.clone(), blob)
                    .map(move |id| {
                        debug!(ctx.logger(), "Saved a raw bundle2 content: {:?}", id);
                        Some(id)
                    })
                    .boxify()
            }
            None => ok(None).boxify(),
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

    // Parse b2x:commonheads
    // This part sent by pushrebase so that server can find out what commits to send back to the
    // client. This part is used as a marker that this push is pushrebase.
    fn maybe_resolve_commonheads(
        &self,
        bundle2: BoxStream<Bundle2Item, Error>,
    ) -> BoxFuture<(Option<CommonHeads>, BoxStream<Bundle2Item, Error>), Error> {
        next_item(bundle2)
            .and_then(|(commonheads, bundle2)| match commonheads {
                Some(Bundle2Item::B2xCommonHeads(_header, heads)) => heads
                    .collect()
                    .map(|heads| {
                        let heads = CommonHeads { heads };
                        (Some(heads), bundle2)
                    })
                    .boxify(),
                Some(part) => ok((None, stream::once(Ok(part)).chain(bundle2).boxify())).boxify(),
                _ => err(format_err!("Unexpected Bundle2 stream end")).boxify(),
            })
            .boxify()
    }

    /// Parse pushvars
    /// It is used to store hook arguments.
    fn maybe_resolve_pushvars(
        &self,
        bundle2: BoxStream<Bundle2Item, Error>,
    ) -> BoxFuture<
        (
            Option<HashMap<String, Bytes>>,
            BoxStream<Bundle2Item, Error>,
        ),
        Error,
    > {
        next_item(bundle2)
            .and_then(move |(newpart, bundle2)| match newpart {
                Some(Bundle2Item::Pushvars(header, emptypart)) => {
                    let pushvars = header.aparams().clone();
                    // ignored for now, will be used for hooks
                    emptypart.map(move |_| (Some(pushvars), bundle2)).boxify()
                }
                Some(part) => ok((None, stream::once(Ok(part)).chain(bundle2).boxify())).boxify(),
                None => ok((None, bundle2)).boxify(),
            })
            .context("While resolving Pushvars")
            .from_err()
            .boxify()
    }

    /// Parse changegroup.
    /// The ChangegroupId will be used in the last step for preparing response
    /// The Changesets should be parsed as RevlogChangesets and used for uploading changesets
    /// The Filelogs should be scheduled for uploading to BlobRepo and the Future resolving in
    /// their upload should be used for uploading changesets
    fn maybe_resolve_changegroup(
        &self,
        ctx: CoreContext,
        bundle2: BoxStream<Bundle2Item, Error>,
    ) -> BoxFuture<(Option<ChangegroupPush>, BoxStream<Bundle2Item, Error>), Error> {
        let repo = self.repo.clone();

        next_item(bundle2)
            .and_then(move |(changegroup, bundle2)| match changegroup {
                // XXX: we may be interested in checking that this is a correct changegroup part
                // type
                Some(Bundle2Item::Changegroup(header, parts))
                | Some(Bundle2Item::B2xInfinitepush(header, parts))
                | Some(Bundle2Item::B2xRebase(header, parts)) => {
                    let part_id = header.part_id();
                    let draft = *header.part_type() == PartHeaderType::B2xInfinitepush;
                    let (c, f) = split_changegroup(parts);
                    convert_to_revlog_changesets(c)
                        .collect()
                        .and_then(move |changesets| {
                            upload_hg_blobs(
                                ctx.clone(),
                                Arc::new(repo.clone()),
                                convert_to_revlog_filelog(ctx.clone(), Arc::new(repo), f),
                                UploadBlobsType::EnsureNoDuplicates,
                            )
                            .map(move |upload_map| {
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
                                mparams: header.mparams().clone(),
                                draft,
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
                    let namespace = try_boxfuture!(header
                        .mparams()
                        .get("namespace")
                        .ok_or(format_err!("pushkey: `namespace` parameter is not set")));

                    let pushkey = match &namespace[..] {
                        b"phases" => Pushkey::Phases,
                        b"bookmarks" => {
                            let part_id = header.part_id();
                            let mparams = header.mparams();
                            let name = try_boxfuture!(get_ascii_param(mparams, "key"));
                            let name = Bookmark::new_ascii(name);
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
                            ))
                            .boxify();
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
        ctx: CoreContext,
        bundle2: BoxStream<Bundle2Item, Error>,
    ) -> BoxFuture<(Manifests, BoxStream<Bundle2Item, Error>), Error> {
        let repo = self.repo.clone();

        next_item(bundle2)
            .and_then(move |(b2xtreegroup2, bundle2)| match b2xtreegroup2 {
                Some(Bundle2Item::B2xTreegroup2(_, parts))
                | Some(Bundle2Item::B2xRebasePack(_, parts)) => upload_hg_blobs(
                    ctx,
                    Arc::new(repo),
                    TreemanifestBundle2Parser::new(parts),
                    UploadBlobsType::IgnoreDuplicates,
                )
                .context("While uploading Manifest Blobs")
                .from_err()
                .map(move |manifests| (manifests, bundle2))
                .boxify(),
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
                    ))
                    .boxify(),
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
        ctx: CoreContext,
        cg_push: ChangegroupPush,
        manifests: Manifests,
    ) -> BoxFuture<(), Error> {
        let changesets = cg_push.changesets;
        let filelogs = cg_push.filelogs;
        let content_blobs = cg_push.content_blobs;
        let draft = cg_push.draft;

        self.ctx
            .scuba()
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
            ctx: CoreContext,
            repo: BlobRepo,
            scuba_logger: ScubaSampleBuilder,
            node: HgChangesetId,
            revlog_cs: RevlogChangeset,
            mut uploaded_changesets: UploadedChangesets,
            filelogs: &Filelogs,
            manifests: &Manifests,
            content_blobs: &ContentBlobs,
            draft: bool,
        ) -> BoxFuture<UploadedChangesets, Error> {
            let (p1, p2) = {
                (
                    get_parent(ctx.clone(), &repo, &uploaded_changesets, revlog_cs.p1),
                    get_parent(ctx.clone(), &repo, &uploaded_changesets, revlog_cs.p2),
                )
            };
            let NewBlobs {
                root_manifest,
                sub_entries,
                // XXX use these content blobs in the future
                content_blobs: _content_blobs,
            } = try_boxfuture!(NewBlobs::new(
                revlog_cs.manifestid(),
                &manifests,
                &filelogs,
                &content_blobs,
                repo.clone(),
            ));

            // DO NOT replace and_then() with join() or futures_ordered()!
            // It may result in a combinatoral explosion in mergy repos (see D14100259)
            p1.and_then(|p1| p2.map(|p2| (p1, p2)))
                .with_context(move |_| format!("While fetching parents for Changeset {}", node))
                .from_err()
                .and_then(move |(p1, p2)| {
                    let cs_metadata = ChangesetMetadata {
                        user: String::from_utf8(revlog_cs.user().into())?,
                        time: revlog_cs.time().clone(),
                        extra: revlog_cs.extra().clone(),
                        comments: String::from_utf8(revlog_cs.comments().into())?,
                    };
                    let create_changeset = CreateChangeset {
                        expected_nodeid: Some(node.into_nodehash()),
                        expected_files: Some(Vec::from(revlog_cs.files())),
                        p1,
                        p2,
                        root_manifest,
                        sub_entries,
                        // XXX pass content blobs to CreateChangeset here
                        cs_metadata,
                        must_check_case_conflicts: true,
                        draft,
                    };
                    let scheduled_uploading = create_changeset.create(ctx, &repo, scuba_logger);

                    uploaded_changesets.insert(node, scheduled_uploading);
                    Ok(uploaded_changesets)
                })
                .boxify()
        }

        let repo = self.repo.clone();

        let changesets_hashes: Vec<_> = changesets.iter().map(|(hash, _)| *hash).collect();

        trace!(self.ctx.logger(), "changesets: {:?}", changesets);
        trace!(self.ctx.logger(), "filelogs: {:?}", filelogs.keys());
        trace!(self.ctx.logger(), "manifests: {:?}", manifests.keys());
        trace!(
            self.ctx.logger(),
            "content blobs: {:?}",
            content_blobs.keys()
        );

        let scuba_logger = self.ctx.scuba().clone();
        stream::iter_ok(changesets)
            .fold(
                HashMap::new(),
                move |uploaded_changesets, (node, revlog_cs)| {
                    upload_changeset(
                        ctx.clone(),
                        repo.clone(),
                        scuba_logger.clone(),
                        node.clone(),
                        revlog_cs,
                        uploaded_changesets,
                        &filelogs,
                        &manifests,
                        &content_blobs,
                        draft,
                    )
                },
            )
            .and_then(|uploaded_changesets| {
                stream::futures_unordered(
                    uploaded_changesets
                        .into_iter()
                        .map(|(_, cs)| cs.get_completed_changeset()),
                )
                .map_err(Error::from)
                .for_each(|_| Ok(()))
            })
            .chain_err(ErrorKind::WhileUploadingData(changesets_hashes))
            .from_err()
            .boxify()
    }

    fn log_commits_to_scribe(
        &self,
        ctx: CoreContext,
        changesets: Vec<ChangesetId>,
    ) -> BoxFuture<(), Error> {
        let repo = self.repo.clone();
        let queue = self.scribe_commit_queue.clone();
        let futs = changesets.into_iter().map(move |changeset_id| {
            cloned!(ctx, repo, queue, changeset_id);
            let generation = repo
                .get_generation_number_by_bonsai(ctx.clone(), changeset_id)
                .and_then(|maybe_gen| maybe_gen.ok_or(err_msg("No generation number found")));
            let parents = repo.get_changeset_parents_by_bonsai(ctx, changeset_id);
            let repo_id = repo.get_repoid();
            let queue = queue;

            generation
                .join(parents)
                .and_then(move |(generation, parents)| {
                    let ci = scribe_commit_queue::CommitInfo::new(
                        repo_id,
                        generation,
                        changeset_id,
                        parents,
                    );
                    queue.queue_commit(ci)
                })
        });
        future::join_all(futs).map(|_| ()).boxify()
    }

    /// Ensures that the next item in stream is None
    fn ensure_stream_finished(
        &self,
        bundle2: BoxStream<Bundle2Item, Error>,
        maybe_full_content: Option<Arc<Mutex<Bytes>>>,
    ) -> BoxFuture<Option<RawBundle2Id>, Error> {
        next_item(bundle2)
            .and_then(|(none, _)| {
                ensure_msg!(none.is_none(), "Expected end of Bundle2");
                Ok(())
            })
            .and_then({
                let resolver = self.clone();
                move |_| {
                    resolver
                        .maybe_save_full_content_bundle2(maybe_full_content)
                        .and_then(move |maybe_raw_bundle2_id| ok(maybe_raw_bundle2_id).boxify())
                        .boxify()
                }
            })
            .boxify()
    }

    /// Takes a changegroup id and prepares a Bytes response containing Bundle2 with reply to
    /// changegroup part saying that the push was successful
    fn prepare_push_response(
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

    fn prepare_pushrebase_response(
        &self,
        ctx: CoreContext,
        commonheads: CommonHeads,
        pushrebased_rev: ChangesetId,
        pushrebased_changesets: Vec<pushrebase::PushrebaseChangesetPair>,
        onto: Bookmark,
        lca_hint: Arc<dyn LeastCommonAncestorsHint>,
        phases_hint: Arc<dyn Phases>,
        bookmark_push_part_id: Option<PartId>,
    ) -> impl Future<Item = Bytes, Error = Error> {
        // Send to the client both pushrebased commit and current "onto" bookmark. Normally they
        // should be the same, however they might be different if bookmark
        // suddenly moved before current pushrebase finished.
        let repo = self.repo.clone();
        let common = commonheads.heads;
        let maybe_onto_head = repo.get_bookmark(ctx.clone(), &onto);

        // write phase as public for this commit
        let pushrebased_rev = phases::mark_reachable_as_public(
            ctx.clone(),
            repo.clone(),
            phases_hint.clone(),
            &[pushrebased_rev.clone()],
        )
        .and_then({
            cloned!(ctx, repo);
            move |_| repo.get_hg_from_bonsai_changeset(ctx, pushrebased_rev)
        });

        let bookmark_reply_part = match bookmark_push_part_id {
            Some(part_id) => Some(try_boxfuture!(parts::replypushkey_part(true, part_id))),
            None => None,
        };

        let obsmarkers_part = match self.pushrebase.emit_obsmarkers {
            true => try_boxfuture!(obsolete::pushrebased_changesets_to_obsmarkers_part(
                ctx.clone(),
                &repo,
                pushrebased_changesets,
            )
            .transpose()),
            false => None,
        };

        let mut scuba_logger = self.ctx.scuba().clone();
        maybe_onto_head
            .join(pushrebased_rev)
            .and_then(move |(maybe_onto_head, pushrebased_rev)| {
                let mut heads = vec![];
                if let Some(onto_head) = maybe_onto_head {
                    heads.push(onto_head);
                }
                heads.push(pushrebased_rev);
                getbundle_response::create_getbundle_response(
                    ctx,
                    repo,
                    common,
                    heads,
                    lca_hint,
                    Some(phases_hint),
                )
            })
            .and_then(move |mut cg_part_builder| {
                cg_part_builder.extend(bookmark_reply_part.into_iter());
                cg_part_builder.extend(obsmarkers_part.into_iter());
                let compression = None;
                create_bundle_stream(cg_part_builder, compression)
                    .collect()
                    .map(|chunks| {
                        let mut total_capacity = 0;
                        for c in chunks.iter() {
                            total_capacity += c.len();
                        }

                        // TODO(stash): make push and pushrebase response streamable - T34090105
                        let mut res = BytesMut::with_capacity(total_capacity);
                        for c in chunks {
                            res.extend_from_slice(&c);
                        }
                        res.freeze()
                    })
                    .context("While preparing response")
                    .from_err()
            })
            .timed({
                move |stats, result| {
                    if result.is_ok() {
                        scuba_logger
                            .add_future_stats(&stats)
                            .log_with_msg("Pushrebase: prepared the response", None);
                    }
                    Ok(())
                }
            })
    }

    fn prepare_push_bookmark_response(
        &self,
        _ctx: CoreContext,
        bookmark_push_part_id: PartId,
        success: bool,
    ) -> impl Future<Item = Bytes, Error = Error> {
        let writer = Cursor::new(Vec::new());
        let mut bundle = Bundle2EncodeBuilder::new(writer);
        // Mercurial currently hangs while trying to read compressed bundles over the wire:
        // https://bz.mercurial-scm.org/show_bug.cgi?id=5646
        // TODO: possibly enable compression support once this is fixed.
        bundle.set_compressor_type(None);
        bundle.add_part(try_boxfuture!(parts::replypushkey_part(
            success,
            bookmark_push_part_id
        )));
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
        Func: FnMut(
                &Self,
                BoxStream<Bundle2Item, Error>,
            ) -> BoxFuture<(Option<T>, BoxStream<Bundle2Item, Error>), Error>
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
        })
        .boxify()
    }

    fn pushrebase(
        &self,
        ctx: CoreContext,
        changesets: Changesets,
        onto_bookmark: &pushrebase::OntoBookmarkParams,
        maybe_raw_bundle2_id: Option<RawBundle2Id>,
    ) -> impl Future<Item = (ChangesetId, Vec<pushrebase::PushrebaseChangesetPair>), Error = Error>
    {
        let bookmark = &onto_bookmark.bookmark;
        let pushrebase = {
            let mut params = self.pushrebase.clone();
            if let Some(rewritedates) = self.bookmark_attrs.should_rewrite_dates(bookmark) {
                // Bookmark config overrides repo pushrebase.rewritedates config
                params.rewritedates = rewritedates;
            }
            params
        };

        let user = ctx.user_unix_name();
        if !self.bookmark_attrs.is_allowed_user(user, bookmark) {
            return future::err(format_err!(
                "[pushrebase] This user `{:?}` is not allowed to move `{:?}`",
                user,
                bookmark
            ))
            .boxify();
        }

        let block_merges = pushrebase.block_merges.clone();
        if block_merges
            && changesets
                .iter()
                .any(|(_, revlog_cs)| revlog_cs.p1.is_some() && revlog_cs.p2.is_some())
        {
            return future::err(format_err!(
                "Pushrebase blocked because it contains a merge commit.\n\
                 If you need this for a specific use case please contact\n\
                 the Source Control team at https://fburl.com/27qnuyl2"
            ))
            .boxify();
        }

        futures::lazy({
            cloned!(self.repo, pushrebase, onto_bookmark);
            move || {
                ctx.scuba().clone().log_with_msg("pushrebase started", None);
                pushrebase::do_pushrebase(
                    ctx,
                    repo,
                    pushrebase,
                    onto_bookmark,
                    changesets
                        .into_iter()
                        .map(|(hg_cs_id, _)| hg_cs_id)
                        .collect(),
                    maybe_raw_bundle2_id,
                )
            }
        })
        .map_err(|err| err_msg(format!("pushrebase failed {:?}", err)))
        .timed({
            let mut scuba_logger = self.ctx.scuba().clone();
            move |stats, result| {
                if let Ok(res) = result {
                    scuba_logger
                        .add_future_stats(&stats)
                        .add("pushrebase_retry_num", res.retry_num)
                        .log_with_msg("Pushrebase finished", None);
                }
                Ok(())
            }
        })
        .map(|res| (res.head, res.rebased_changesets))
        .boxify()
    }

    fn run_hooks(
        &self,
        ctx: CoreContext,
        changesets: Changesets,
        pushvars: Option<HashMap<String, Bytes>>,
        onto_bookmark: &Bookmark,
    ) -> BoxFuture<(), RunHooksError> {
        // TODO: should we also accept the Option<BookmarkPush> and run hooks on that?
        let mut futs = stream::FuturesUnordered::new();
        for (hg_cs_id, _) in changesets {
            futs.push(
                self.hook_manager
                    .run_changeset_hooks_for_bookmark(
                        ctx.clone(),
                        hg_cs_id.clone(),
                        onto_bookmark,
                        pushvars.clone(),
                    )
                    .join(self.hook_manager.run_file_hooks_for_bookmark(
                        ctx.clone(),
                        hg_cs_id,
                        onto_bookmark,
                        pushvars.clone(),
                    )),
            )
        }
        futs.collect()
            .from_err()
            .and_then(|res| {
                let (cs_hook_results, file_hook_results): (Vec<_>, Vec<_>) =
                    res.into_iter().unzip();
                let cs_hook_failures: Vec<(ChangesetHookExecutionID, HookExecution)> =
                    cs_hook_results
                        .into_iter()
                        .flatten()
                        .filter(|(_, exec)| match exec {
                            HookExecution::Accepted => false,
                            HookExecution::Rejected(_) => true,
                        })
                        .collect();
                let file_hook_failures: Vec<(FileHookExecutionID, HookExecution)> =
                    file_hook_results
                        .into_iter()
                        .flatten()
                        .filter(|(_, exec)| match exec {
                            HookExecution::Accepted => false,
                            HookExecution::Rejected(_) => true,
                        })
                        .collect();
                if cs_hook_failures.len() > 0 || file_hook_failures.len() > 0 {
                    Err(RunHooksError::Failures((
                        cs_hook_failures,
                        file_hook_failures,
                    )))
                } else {
                    Ok(())
                }
            })
            .boxify()
    }
}

#[derive(Debug)]
pub enum RunHooksError {
    Failures(
        (
            Vec<(ChangesetHookExecutionID, HookExecution)>,
            Vec<(FileHookExecutionID, HookExecution)>,
        ),
    ),
    Error(Error),
}

impl From<Error> for RunHooksError {
    fn from(error: Error) -> Self {
        RunHooksError::Error(error)
    }
}

fn check_bookmark_push_allowed(
    ctx: CoreContext,
    repo: BlobRepo,
    bookmark_attrs: BookmarkAttrs,
    allow_non_fast_forward: bool,
    bp: BonsaiBookmarkPush,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
) -> impl Future<Item = BonsaiBookmarkPush, Error = Error> {
    let user = ctx.user_unix_name();
    if !bookmark_attrs.is_allowed_user(user, &bp.name) {
        return future::err(format_err!(
            "[push] This user `{:?}` is not allowed to move `{:?}`",
            user,
            &bp.name
        ))
        .right_future();
    }

    let fastforward_only_bookmark = bookmark_attrs.is_fast_forward_only(&bp.name);
    // only allow non fast forward moves if the pushvar is set and the bookmark does not
    // explicitly block them.
    let block_non_fast_forward = fastforward_only_bookmark || !allow_non_fast_forward;

    match (bp.old, bp.new) {
        (Some(old), Some(new)) if block_non_fast_forward && old != new => lca_hint
            .is_ancestor(ctx, repo.get_changeset_fetcher(), old, new)
            .and_then(|is_ancestor| {
                if is_ancestor {
                    Ok(bp)
                } else {
                    Err(format_err!("Non fastforward bookmark move"))
                }
            })
            .left_future(),
        (Some(_old), None) if fastforward_only_bookmark => Err(format_err!(
            "Deletion of bookmark {} is forbidden.",
            bp.name
        ))
        .into_future()
        .right_future(),
        _ => Ok(bp).into_future().right_future(),
    }
}

fn add_bookmark_to_transaction(
    txn: &mut Box<Transaction>,
    bookmark_push: BonsaiBookmarkPush,
    bookmark_update_reason: BookmarkUpdateReason,
) -> Result<()> {
    match (bookmark_push.new, bookmark_push.old) {
        (Some(new), Some(old)) => txn.update(&bookmark_push.name, new, old, bookmark_update_reason),
        (Some(new), None) => txn.create(&bookmark_push.name, new, bookmark_update_reason),
        (None, Some(old)) => txn.delete(&bookmark_push.name, old, bookmark_update_reason),
        _ => Ok(()),
    }
}

/// Retrieves the parent from uploaded changesets, if it is missing then fetches it from BlobRepo
fn get_parent(
    ctx: CoreContext,
    repo: &BlobRepo,
    map: &UploadedChangesets,
    p: Option<HgNodeHash>,
) -> impl Future<Item = Option<ChangesetHandle>, Error = Error> {
    let res = match p {
        None => None,
        Some(p) => match map.get(&HgChangesetId::new(p)) {
            None => Some(ChangesetHandle::ready_cs_handle(
                ctx,
                Arc::new(repo.clone()),
                HgChangesetId::new(p),
            )),
            Some(cs) => Some(cs.clone()),
        },
    };
    ok(res)
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
        repo: BlobRepo,
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

        let (entries, content_blobs, root_manifest) = match manifests.get(&root_key) {
            Some((ref manifest_content, ref p1, ref p2, ref manifest_root)) => {
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
                STATS::per_changeset_content_blobs_count
                    .add_value(counters.content_blobs_count as i64);
                let root_manifest = manifest_root
                    .clone()
                    .map(|it| Some((*it).clone()))
                    .from_err()
                    .boxify();

                (entries, content_blobs, root_manifest)
            }
            None => {
                let entry = (repo.get_root_entry(manifest_root_id), RepoPath::RootPath);
                (vec![], vec![], future::ok(Some(entry)).boxify())
            }
        };

        Ok(Self {
            root_manifest,
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
                    let (mut walked_entries, mut walked_cbinfos, sub_counters) = Self::walk_helper(
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
