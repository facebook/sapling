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
use crate::upload_changesets::upload_changeset;
use ascii::AsciiString;
use blobrepo::{BlobRepo, ChangesetHandle};
use bookmarks::{BookmarkName, BookmarkUpdateReason, BundleReplayData, Transaction};
use bytes::{Bytes, BytesMut};
use cloned::cloned;
use context::CoreContext;
use failure::{err_msg, format_err, Compat};
use failure_ext::{ensure_msg, FutureFailureErrorExt};
use futures::future::{self, err, ok, Shared};
use futures::stream;
use futures::{Future, IntoFuture, Stream};
use futures_ext::{try_boxfuture, BoxFuture, BoxStream, FutureExt, StreamExt};
use futures_stats::Timed;
use hooks::{ChangesetHookExecutionID, FileHookExecutionID, HookExecution, HookManager};
use lazy_static::lazy_static;
use mercurial_bundles::{
    create_bundle_stream, parts, Bundle2EncodeBuilder, Bundle2Item, PartHeader, PartHeaderInner,
    PartHeaderType, PartId,
};
use mercurial_revlog::changeset::RevlogChangeset;
use mercurial_types::{
    blobs::{ContentBlobInfo, HgBlobEntry},
    HgChangesetId, HgNodeKey, RepoPath,
};
use metaconfig_types::{BookmarkAttrs, InfinitepushParams, PushrebaseParams, RepoReadOnly};
use mononoke_types::{BlobstoreValue, ChangesetId, RawBundle2, RawBundle2Id};
use phases::{self, Phases};
use reachabilityindex::LeastCommonAncestorsHint;
use scribe_commit_queue::{self, ScribeCommitQueue};
use scuba_ext::ScubaSampleBuilderExt;
use slog::{debug, o, trace, warn};
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use topo_sort::sort_topological;
use wirepack::{TreemanifestBundle2Parser, TreemanifestEntry};

type Changesets = Vec<(HgChangesetId, RevlogChangeset)>;
type Filelogs = HashMap<HgNodeKey, Shared<BoxFuture<(HgBlobEntry, RepoPath), Compat<Error>>>>;
type ContentBlobs = HashMap<HgNodeKey, ContentBlobInfo>;
type Manifests = HashMap<HgNodeKey, <TreemanifestEntry as UploadableHgBlob>::Value>;

// This is to match the core hg behavior from https://fburl.com/jf3iyl7y
// Mercurial substitutes the `onto` parameter with this bookmark name when
// the force pushrebase is done, so we need to look for it and make sure we
// do the right thing here too.
lazy_static! {
    static ref DONOTREBASEBOOKMARK: BookmarkName =
        BookmarkName::new("__pushrebase_donotrebase__").unwrap();
}

pub enum BundleResolverError {
    HookError(
        (
            Vec<(ChangesetHookExecutionID, HookExecution)>,
            Vec<(FileHookExecutionID, HookExecution)>,
        ),
    ),
    PushrebaseConflicts(Vec<pushrebase::PushrebaseConflict>),
    Error(Error),
}

impl From<Error> for BundleResolverError {
    fn from(error: Error) -> Self {
        Self::Error(error)
    }
}

impl From<BundleResolverError> for Error {
    fn from(error: BundleResolverError) -> Error {
        // DO NOT CHANGE FORMATTING WITHOUT UPDATING https://fburl.com/diffusion/bs9fys78 first!!
        use BundleResolverError::*;
        match error {
            HookError((cs_hook_failures, file_hook_failures)) => {
                let mut err_msgs = vec![];
                for (exec_id, exec_info) in cs_hook_failures {
                    if let HookExecution::Rejected(info) = exec_info {
                        err_msgs.push(format!(
                            "{} for {}: {}",
                            exec_id.hook_name, exec_id.cs_id, info.long_description
                        ));
                    }
                }
                for (exec_id, exec_info) in file_hook_failures {
                    if let HookExecution::Rejected(info) = exec_info {
                        err_msgs.push(format!(
                            "{} for {}: {}",
                            exec_id.hook_name, exec_id.cs_id, info.long_description
                        ));
                    }
                }
                err_msg(format!("hooks failed:\n{}", err_msgs.join("\n")))
            }
            PushrebaseConflicts(conflicts) => {
                err_msg(format!("pushrebase failed Conflicts({:?})", conflicts))
            }
            Error(err) => err,
        }
    }
}

/// The resolve function takes a bundle2, interprets it's content as Changesets, Filelogs and
/// Manifests and uploades all of them to the provided BlobRepo in the correct order.
/// It returns a Future that contains the response that should be send back to the requester.
pub fn resolve(
    ctx: CoreContext,
    repo: BlobRepo,
    pushrebase: PushrebaseParams,
    bookmark_attrs: BookmarkAttrs,
    infinitepush_params: InfinitepushParams,
    bundle2: BoxStream<Bundle2Item, Error>,
    hook_manager: Arc<HookManager>,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    phases_hint: Arc<dyn Phases>,
    readonly: RepoReadOnly,
    maybe_full_content: Option<Arc<Mutex<Bytes>>>,
    pure_push_allowed: bool,
) -> BoxFuture<Bytes, BundleResolverError> {
    let resolver = Bundle2Resolver::new(
        ctx.clone(),
        repo,
        pushrebase,
        bookmark_attrs,
        infinitepush_params,
        hook_manager,
    );
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
        .from_err()
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
                        .from_err()
                        .boxify()
                    } else {
                        fn changegroup_always_unacceptable() -> bool {
                            false
                        };
                        resolve_pushrebase(
                            ctx,
                            commonheads,
                            resolver,
                            bundle2,
                            maybe_pushvars,
                            lca_hint,
                            phases_hint,
                            maybe_full_content,
                            changegroup_always_unacceptable,
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
                        move || pure_push_allowed,
                    )
                    .from_err()
                    .boxify()
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
    changegroup_acceptable: impl FnOnce() -> bool + Send + Sync + 'static,
) -> BoxFuture<Bytes, Error> {
    resolver
        .maybe_resolve_changegroup(ctx.clone(), bundle2, changegroup_acceptable)
        .and_then({
            cloned!(resolver);
            move |(cg_push, bundle2)| {
                resolver
                    .resolve_multiple_parts(bundle2, Bundle2Resolver::maybe_resolve_pushkey)
                    .map(move |(pushkeys, bundle2)| {
                        let infinitepush_bp = cg_push
                            .as_ref()
                            .and_then(|cg_push| cg_push.infinitepush_payload.clone())
                            .and_then(|ip_payload| ip_payload.bookmark_push);
                        let bookmark_push = collect_all_bookmark_pushes(pushkeys, infinitepush_bp);
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
                        .upload_changesets(ctx, cg_push, manifests, false)
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
                    let bookmark_ids: Vec<_> = bookmark_push
                        .iter()
                        .filter_map(|bp| match bp {
                            BookmarkPush::PlainPush(bp) => Some(bp.part_id),
                            BookmarkPush::Infinitepush(..) => None,
                        })
                        .collect();
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

// Enum used to pass data for normal or forceful pushrebases
// Normal pushrebase is what one would expect: take a (potentially
// stack of) commit(s) and rebase it on top of a given bookmark.
// Force pushrebase is basically a push, which for logging
// and respondin purposes is treated like a pushrebase
enum PushrebaseBookmarkSpec {
    NormalPushrebase(pushrebase::OntoBookmarkParams),
    ForcePushrebase(PlainBookmarkPush<HgChangesetId>),
}

impl PushrebaseBookmarkSpec {
    fn get_bookmark_name(&self) -> BookmarkName {
        match self {
            PushrebaseBookmarkSpec::NormalPushrebase(onto_params) => onto_params.bookmark.clone(),
            PushrebaseBookmarkSpec::ForcePushrebase(plain_push) => plain_push.name.clone(),
        }
    }
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
    changegroup_acceptable: impl FnOnce() -> bool + Send + Sync + 'static,
) -> BoxFuture<Bytes, BundleResolverError> {
    resolver
        .resolve_b2xtreegroup2(ctx.clone(), bundle2)
        .and_then({
            cloned!(ctx, resolver);
            move |(manifests, bundle2)| {
                resolver
                    .maybe_resolve_changegroup(ctx, bundle2, changegroup_acceptable)
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
                    let onto_bookmark = BookmarkName::new(onto_bookmark)?;
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
                let will_rebase = onto_params.bookmark != *DONOTREBASEBOOKMARK;
                // Mutation information must not be present in public commits
                // See T54101162, S186586
                if !will_rebase {
                    for (_, hg_cs) in &changesets {
                        for key in pushrebase::MUTATION_KEYS {
                            if hg_cs.extra.as_ref().contains_key(key.as_bytes()) {
                                return future::err(err_msg("Forced push blocked because it contains mutation metadata.\n\
                                                        You can remove the metadata from a commit with `hg amend --config mutation.record=false`.\n\
                                                        For more help, please contact the Source Control team at https://fburl.com/27qnuyl2")).left_future();
                            }
                        }
                    }
                }
                resolver
                    .upload_changesets(
                        ctx,
                        cg_push,
                        manifests,
                        will_rebase,
                    )
                    .map(move |()| (changesets, onto_params, bundle2)).right_future()
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
                            let bookmark_pushes = collect_pushkey_bookmark_pushes(pushkeys);

                            if bookmark_pushes.len() > 1 {
                                return future::err(format_err!(
                                    "Too many pushkey parts: {:?}",
                                    bookmark_pushes
                                ))
                                .boxify();
                            }

                            let (bookmark_push_part_id, bookmark_spec) = match bookmark_pushes
                                .get(0)
                            {
                                Some(bk_push)
                                    if bk_push.name != onto_params.bookmark
                                        && onto_params.bookmark != *DONOTREBASEBOOKMARK =>
                                {
                                    return future::err(format_err!(
                                        "allowed only pushes of {} bookmark: {:?}",
                                        onto_params.bookmark,
                                        bookmark_pushes
                                    ))
                                    .boxify();
                                }
                                Some(bk_push) if onto_params.bookmark == *DONOTREBASEBOOKMARK => {
                                    (
                                        // This is a force pushrebase scenario. We need to ignore `onto_params`
                                        // and run normal push (using bk_push), but generate a pushrebase
                                        // response.
                                        // See comment next to DONOTREBASEBOOKMARK definition
                                        Some(bk_push.part_id),
                                        PushrebaseBookmarkSpec::ForcePushrebase(bk_push.clone()),
                                    )
                                }
                                Some(bk_push) => (
                                    Some(bk_push.part_id),
                                    PushrebaseBookmarkSpec::NormalPushrebase(onto_params),
                                ),
                                None => {
                                    (None, PushrebaseBookmarkSpec::NormalPushrebase(onto_params))
                                }
                            };

                            resolver
                                .ensure_stream_finished(bundle2, maybe_full_content)
                                .map(move |maybe_raw_bundle2_id| {
                                    (
                                        changesets,
                                        bookmark_push_part_id,
                                        bookmark_spec,
                                        maybe_raw_bundle2_id,
                                    )
                                })
                                .boxify()
                        }
                    })
            }
        })
        .from_err()
        .and_then({
            cloned!(ctx, resolver, lca_hint);
            move |(changesets, bookmark_push_part_id, bookmark_spec, maybe_raw_bundle2_id)| {
                let bookmark = bookmark_spec.get_bookmark_name();
                resolver
                    .run_hooks(ctx.clone(), changesets.clone(), maybe_pushvars, &bookmark)
                    .and_then(move |()| {
                        match bookmark_spec {
                            PushrebaseBookmarkSpec::NormalPushrebase(onto_params) => resolver
                                .pushrebase(ctx, changesets, &onto_params, maybe_raw_bundle2_id)
                                .left_future(),
                            PushrebaseBookmarkSpec::ForcePushrebase(plain_push) => resolver
                                .force_pushrebase(ctx, lca_hint, plain_push, maybe_raw_bundle2_id)
                                .from_err()
                                .right_future(),
                        }
                        .map(move |pushrebased_rev| {
                            (pushrebased_rev, bookmark, bookmark_push_part_id)
                        })
                    })
            }
        })
        .and_then(
            move |((pushrebased_rev, pushrebased_changesets), bookmark, bookmark_push_part_id)| {
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
                            bookmark,
                            lca_hint,
                            phases_hint,
                            bookmark_push_part_id,
                        )
                    })
                    .from_err()
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
                let bookmark_pushes = collect_pushkey_bookmark_pushes(pushkeys);

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
                let pushes = vec![BookmarkPush::PlainPush(bookmark_push)];
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

enum BookmarkPush<T: Copy> {
    PlainPush(PlainBookmarkPush<T>),
    Infinitepush(InfiniteBookmarkPush<T>),
}

#[derive(Debug, Clone)]
struct PlainBookmarkPush<T: Copy> {
    part_id: PartId,
    name: BookmarkName,
    old: Option<T>,
    new: Option<T>,
}

#[derive(Debug, Clone)]
struct InfiniteBookmarkPush<T> {
    name: BookmarkName,
    create: bool,
    force: bool,
    old: Option<T>,
    new: T,
}

#[derive(Debug, Clone)]
struct InfinitepushPayload {
    /// An Infinitepush bookmark (aka scratch bookmark) that was provided through an Infinitepush
    /// bundle part.
    bookmark_push: Option<InfiniteBookmarkPush<HgChangesetId>>,
}

struct ChangegroupPush {
    part_id: PartId,
    changesets: Changesets,
    filelogs: Filelogs,
    content_blobs: ContentBlobs,
    mparams: HashMap<String, Bytes>,
    /// Infinitepush data provided through the Changegroup. If the push was an Infinitepush, this
    /// will be present.
    infinitepush_payload: Option<InfinitepushPayload>,
}

struct CommonHeads {
    heads: Vec<HgChangesetId>,
}

enum Pushkey {
    HgBookmarkPush(PlainBookmarkPush<HgChangesetId>),
    Phases,
}

// TODO: (torozco) T44841164 bonsai_from_hg_opt should probably error if we map Some<HgChangesetId>
// to None.
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

fn hg_bookmark_push_to_bonsai(
    ctx: CoreContext,
    repo: &BlobRepo,
    bookmark_push: BookmarkPush<HgChangesetId>,
) -> impl Future<Item = BookmarkPush<ChangesetId>, Error = Error> + Send {
    match bookmark_push {
        BookmarkPush::PlainPush(PlainBookmarkPush {
            part_id,
            name,
            old,
            new,
        }) => (
            bonsai_from_hg_opt(ctx.clone(), repo, old),
            bonsai_from_hg_opt(ctx, repo, new),
        )
            .into_future()
            .map(move |(old, new)| {
                BookmarkPush::PlainPush(PlainBookmarkPush {
                    part_id,
                    name,
                    old,
                    new,
                })
            })
            .left_future(),
        BookmarkPush::Infinitepush(InfiniteBookmarkPush {
            name,
            force,
            create,
            old,
            new,
        }) => (
            bonsai_from_hg_opt(ctx.clone(), repo, old),
            repo.get_bonsai_from_hg(ctx.clone(), new),
        )
            .into_future()
            .and_then(|(old, new)| match new {
                Some(new) => Ok((old, new)),
                None => Err(err_msg("Bonsai Changeset not found")),
            })
            .map(move |(old, new)| {
                BookmarkPush::Infinitepush(InfiniteBookmarkPush {
                    name,
                    force,
                    create,
                    old,
                    new,
                })
            })
            .right_future(),
    }
}

/// Holds repo and logger for convienience access from it's methods
#[derive(Clone)]
struct Bundle2Resolver {
    ctx: CoreContext,
    repo: BlobRepo,
    pushrebase: PushrebaseParams,
    bookmark_attrs: BookmarkAttrs,
    infinitepush_params: InfinitepushParams,
    hook_manager: Arc<HookManager>,
    scribe_commit_queue: Arc<dyn ScribeCommitQueue>,
}

impl Bundle2Resolver {
    fn new(
        ctx: CoreContext,
        repo: BlobRepo,
        pushrebase: PushrebaseParams,
        bookmark_attrs: BookmarkAttrs,
        infinitepush_params: InfinitepushParams,
        hook_manager: Arc<HookManager>,
    ) -> Self {
        let scribe_commit_queue = match pushrebase.commit_scribe_category.clone() {
            Some(category) => Arc::new(scribe_commit_queue::LogToScribe::new_with_default_scribe(
                ctx.fb, category,
            )),
            None => Arc::new(scribe_commit_queue::LogToScribe::new_with_discard()),
        };

        Self {
            ctx,
            repo,
            pushrebase,
            bookmark_attrs,
            infinitepush_params,
            hook_manager,
            scribe_commit_queue,
        }
    }

    /// Produce a future that creates a transaction with potentitally multiple bookmark pushes
    fn resolve_bookmark_pushes(
        &self,
        bookmark_pushes: Vec<BookmarkPush<HgChangesetId>>,
        reason: BookmarkUpdateReason,
        lca_hint: Arc<dyn LeastCommonAncestorsHint>,
        allow_non_fast_forward: bool,
    ) -> impl Future<Item = (), Error = Error> {
        let resolver = self.clone();
        let ctx = resolver.ctx.clone();
        let repo = resolver.repo.clone();
        let bookmark_attrs = resolver.bookmark_attrs.clone();
        let infinitepush_params = resolver.infinitepush_params.clone();

        let bookmarks_push_fut = bookmark_pushes
            .into_iter()
            .map(move |bp| {
                hg_bookmark_push_to_bonsai(ctx.clone(), &repo, bp).and_then({
                    cloned!(repo, ctx, lca_hint, bookmark_attrs, infinitepush_params);
                    move |bp| match bp {
                        BookmarkPush::PlainPush(bp) => check_bookmark_push_allowed(
                            ctx.clone(),
                            repo.clone(),
                            bookmark_attrs,
                            allow_non_fast_forward,
                            infinitepush_params,
                            bp,
                            lca_hint,
                        )
                        .map(|bp| Some(BookmarkPush::PlainPush(bp)))
                        .left_future(),
                        BookmarkPush::Infinitepush(bp) => filter_or_check_infinitepush_allowed(
                            ctx.clone(),
                            repo.clone(),
                            lca_hint,
                            infinitepush_params,
                            bp,
                        )
                        .map(|maybe_bp| maybe_bp.map(BookmarkPush::Infinitepush))
                        .right_future(),
                    }
                })
            })
            .collect::<Vec<_>>();

        future::join_all(bookmarks_push_fut).and_then({
            cloned!(resolver);
            move |bonsai_bookmark_pushes| {
                if bonsai_bookmark_pushes.is_empty() {
                    // If we have no bookmarks, then don't create an empty transaction. This is a
                    // temporary workaround for the fact that we committing an empty transaction
                    // evicts the cache.
                    return ok(()).boxify();
                }

                let mut txn = resolver
                    .repo
                    .update_bookmark_transaction(resolver.ctx.clone());

                for bp in bonsai_bookmark_pushes.into_iter().flatten() {
                    try_boxfuture!(add_bookmark_to_transaction(&mut txn, bp, reason.clone()));
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
                        return_with_rest_of_bundle(false, part, bundle2)
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
                    .upload_blob(ctx.clone(), blob)
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
                Some(part) => return_with_rest_of_bundle(None, part, bundle2),
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
                Some(part) => return_with_rest_of_bundle(None, part, bundle2),
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
    /// `pure_push_allowed` argument is responsible for allowing
    /// pure (non-pushrebase and non-infinitepush) pushes
    fn maybe_resolve_changegroup(
        &self,
        ctx: CoreContext,
        bundle2: BoxStream<Bundle2Item, Error>,
        changegroup_acceptable: impl FnOnce() -> bool + Send + Sync + 'static,
    ) -> BoxFuture<(Option<ChangegroupPush>, BoxStream<Bundle2Item, Error>), Error> {
        let repo = self.repo.clone();

        let fut = next_item(bundle2).and_then(move |(changegroup, bundle2)| match changegroup {
            // XXX: we may be interested in checking that this is a correct changegroup part
            // type
            Some(Bundle2Item::Changegroup(header, parts))
            | Some(Bundle2Item::B2xInfinitepush(header, parts))
            | Some(Bundle2Item::B2xRebase(header, parts)) => {
                if header.part_type() == &PartHeaderType::Changegroup && !changegroup_acceptable() {
                    // Changegroup part type signals that we are in a pure push scenario
                    return err(format_err!("Pure pushes are disallowed in this repo")).boxify();
                }
                let (c, f) = split_changegroup(parts);
                convert_to_revlog_changesets(c)
                    .collect()
                    .and_then({
                        cloned!(repo, ctx);
                        move |changesets| {
                            upload_hg_blobs(
                                ctx.clone(),
                                repo.clone(),
                                convert_to_revlog_filelog(ctx.clone(), repo.clone(), f),
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
                        }
                    })
                    .and_then({
                        cloned!(ctx, repo);
                        move |(changesets, filelogs, content_blobs)| {
                            build_changegroup_push(
                                ctx,
                                &repo,
                                header,
                                changesets,
                                filelogs,
                                content_blobs,
                            )
                            .map(move |cg_push| (Some(cg_push), bundle2))
                        }
                    })
                    .boxify()
            }
            Some(part) => return_with_rest_of_bundle(None, part, bundle2),
            _ => err(format_err!("Unexpected Bundle2 stream end")).boxify(),
        });

        // Check that infinitepush is enabled if we use it.
        let fut = if self.infinitepush_params.allow_writes {
            fut.left_future()
        } else {
            fut.and_then(|maybe_cg_push| match maybe_cg_push {
                (Some(ref cg_push), _) if cg_push.infinitepush_payload.is_some() => {
                    let m =
                        "Infinitepush is not enabled on this server. Contact Source Control @ FB.";
                    Err(err_msg(m))
                }
                r => Ok(r),
            })
            .right_future()
        };

        fut.context("While resolving Changegroup")
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
                            let name = BookmarkName::new_ascii(name);
                            let old = try_boxfuture!(get_optional_changeset_param(mparams, "old"));
                            let new = try_boxfuture!(get_optional_changeset_param(mparams, "new"));

                            Pushkey::HgBookmarkPush(PlainBookmarkPush {
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
                Some(part) => return_with_rest_of_bundle(None, part, bundle2),
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
                    repo,
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
        force_draft: bool,
    ) -> BoxFuture<(), Error> {
        let changesets = try_boxfuture!(toposort_changesets(cg_push.changesets));
        let filelogs = cg_push.filelogs;
        let content_blobs = cg_push.content_blobs;
        let draft = force_draft || cg_push.infinitepush_payload.is_some();

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
        let upload_changeset_fun = Arc::new({
            cloned!(ctx);
            move |uploaded_changesets: HashMap<HgChangesetId, ChangesetHandle>,
                  node: HgChangesetId,
                  revlog_cs: RevlogChangeset|
                  -> BoxFuture<HashMap<HgChangesetId, ChangesetHandle>, Error> {
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
            }
        });

        // Each commit gets a future. This future polls futures of parent commits, which poll futures
        // of their parents and so on. However that might cause stackoverflow on very large pushes
        // To avoid it we commit changesets in relatively small chunks.
        let chunk_size = 100;
        stream::iter_ok(changesets)
            .chunks(chunk_size)
            .for_each(move |chunk| {
                stream::iter_ok(chunk)
                    .fold(HashMap::new(), {
                        cloned!(upload_changeset_fun);
                        move |uploaded_changesets, (node, revlog_cs)| {
                            (*upload_changeset_fun)(uploaded_changesets, node, revlog_cs)
                        }
                    })
                    .and_then({
                        move |uploaded_changesets| {
                            stream::iter_ok(
                                uploaded_changesets
                                    .into_iter()
                                    .map(move |(_, handle)| handle.get_completed_changeset()),
                            )
                            .buffered(chunk_size)
                            .map_err(Error::from)
                            .for_each(|_| Ok(()))
                        }
                    })
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
        onto: BookmarkName,
        lca_hint: Arc<dyn LeastCommonAncestorsHint>,
        phases: Arc<dyn Phases>,
        bookmark_push_part_id: Option<PartId>,
    ) -> impl Future<Item = Bytes, Error = Error> {
        // Send to the client both pushrebased commit and current "onto" bookmark. Normally they
        // should be the same, however they might be different if bookmark
        // suddenly moved before current pushrebase finished.
        let repo = self.repo.clone();
        let common = commonheads.heads;
        let maybe_onto_head = repo.get_bookmark(ctx.clone(), &onto);

        // write phase as public for this commit
        let pushrebased_rev = phases
            .add_reachable_as_public(ctx.clone(), repo.clone(), vec![pushrebased_rev.clone()])
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
                    Some(phases),
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

    fn force_pushrebase(
        &self,
        ctx: CoreContext,
        lca_hint: Arc<dyn LeastCommonAncestorsHint>,
        bookmark_push: PlainBookmarkPush<HgChangesetId>,
        maybe_raw_bundle2_id: Option<RawBundle2Id>,
    ) -> impl Future<Item = (ChangesetId, Vec<pushrebase::PushrebaseChangesetPair>), Error = Error>
    {
        let this = self.clone();
        bonsai_from_hg_opt(ctx.clone(), &self.repo, bookmark_push.new.clone()).and_then(
            move |maybe_target_bcs| {
                let target_bcs = try_boxfuture!(maybe_target_bcs
                    .ok_or(err_msg("new changeset is required for force pushrebase")));
                let pushes = vec![BookmarkPush::PlainPush(bookmark_push)];
                let reason = BookmarkUpdateReason::Pushrebase {
                    bundle_replay_data: maybe_raw_bundle2_id.map(|id| BundleReplayData::new(id)),
                };
                // Note that this push did not do any actual rebases, so we do not
                // need to provide any actual mapping, an empty Vec will do
                let ret = (target_bcs, Vec::new());
                this.resolve_bookmark_pushes(pushes, reason, lca_hint, true)
                    .map(move |_| ret)
                    .boxify()
            },
        )
    }

    fn pushrebase(
        &self,
        ctx: CoreContext,
        changesets: Changesets,
        onto_bookmark: &pushrebase::OntoBookmarkParams,
        maybe_raw_bundle2_id: Option<RawBundle2Id>,
    ) -> impl Future<
        Item = (ChangesetId, Vec<pushrebase::PushrebaseChangesetPair>),
        Error = BundleResolverError,
    > {
        let bookmark = &onto_bookmark.bookmark;
        let pushrebase = {
            let mut params = self.pushrebase.clone();
            if let Some(rewritedates) = self.bookmark_attrs.should_rewrite_dates(bookmark) {
                // Bookmark config overrides repo pushrebase.rewritedates config
                params.rewritedates = rewritedates;
            }
            params
        };

        if let Err(error) = check_plain_bookmark_move_preconditions(
            &ctx,
            &bookmark,
            "pushrebase",
            &self.bookmark_attrs,
            &self.infinitepush_params,
        ) {
            return future::err(error).from_err().boxify();
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
            .from_err()
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
        .map_err(|err| match err {
            pushrebase::PushrebaseError::Conflicts(conflicts) => {
                BundleResolverError::PushrebaseConflicts(conflicts)
            }
            _ => BundleResolverError::Error(err_msg(format!("pushrebase failed {:?}", err))),
        })
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
        onto_bookmark: &BookmarkName,
    ) -> BoxFuture<(), BundleResolverError> {
        // TODO: should we also accept the Option<HgBookmarkPush> and run hooks on that?
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
                    Err(BundleResolverError::HookError((
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

fn check_is_ancestor_opt(
    ctx: CoreContext,
    repo: BlobRepo,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    old: Option<ChangesetId>,
    new: ChangesetId,
) -> impl Future<Item = (), Error = Error> {
    match old {
        None => ok(()).left_future(),
        Some(old) => {
            if old == new {
                ok(()).left_future()
            } else {
                lca_hint
                    .is_ancestor(ctx, repo.get_changeset_fetcher(), old, new)
                    .and_then(|is_ancestor| match is_ancestor {
                        true => Ok(()),
                        false => Err(format_err!("Non fastforward bookmark move")),
                    })
                    .right_future()
            }
        }
        .right_future(),
    }
}

fn check_plain_bookmark_move_preconditions(
    ctx: &CoreContext,
    bookmark: &BookmarkName,
    reason: &'static str,
    bookmark_attrs: &BookmarkAttrs,
    infinitepush_params: &InfinitepushParams,
) -> Result<()> {
    let user = ctx.user_unix_name();
    if !bookmark_attrs.is_allowed_user(user, bookmark) {
        return Err(format_err!(
            "[{}] This user `{:?}` is not allowed to move `{:?}`",
            reason,
            user,
            bookmark
        ));
    }

    if let Some(ref namespace) = infinitepush_params.namespace {
        if namespace.matches_bookmark(bookmark) {
            return Err(format_err!(
                "[{}] Only Infinitepush bookmarks are allowed to match pattern {}",
                reason,
                namespace.as_str(),
            ));
        }
    }

    Ok(())
}

fn check_bookmark_push_allowed(
    ctx: CoreContext,
    repo: BlobRepo,
    bookmark_attrs: BookmarkAttrs,
    allow_non_fast_forward: bool,
    infinitepush_params: InfinitepushParams,
    bp: PlainBookmarkPush<ChangesetId>,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
) -> impl Future<Item = PlainBookmarkPush<ChangesetId>, Error = Error> {
    if let Err(error) = check_plain_bookmark_move_preconditions(
        &ctx,
        &bp.name,
        "push",
        &bookmark_attrs,
        &infinitepush_params,
    ) {
        return future::err(error).right_future();
    }

    let fastforward_only_bookmark = bookmark_attrs.is_fast_forward_only(&bp.name);
    // only allow non fast forward moves if the pushvar is set and the bookmark does not
    // explicitly block them.
    let block_non_fast_forward = fastforward_only_bookmark || !allow_non_fast_forward;

    match (bp.old, bp.new) {
        (old, Some(new)) if block_non_fast_forward => {
            check_is_ancestor_opt(ctx, repo, lca_hint, old, new)
                .map(|_| bp)
                .left_future()
        }
        (Some(_old), None) if fastforward_only_bookmark => Err(format_err!(
            "Deletion of bookmark {} is forbidden.",
            bp.name
        ))
        .into_future()
        .right_future(),
        _ => Ok(bp).into_future().right_future(),
    }
}

fn filter_or_check_infinitepush_allowed(
    ctx: CoreContext,
    repo: BlobRepo,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    infinitepush_params: InfinitepushParams,
    bp: InfiniteBookmarkPush<ChangesetId>,
) -> impl Future<Item = Option<InfiniteBookmarkPush<ChangesetId>>, Error = Error> {
    match infinitepush_params.namespace {
        Some(namespace) => ok(bp)
            // First, check that we match the namespace.
            .and_then(move |bp| match namespace.matches_bookmark(&bp.name) {
                true => ok(bp),
                false => err(format_err!(
                    "Invalid Infinitepush bookmark: {} (Infinitepush bookmarks must match pattern {})",
                    &bp.name,
                    namespace.as_str()
                ))
            })
            // Now, check that the bookmark we want to update either exists or is being created.
            .and_then(|bp| {
                if bp.old.is_some() || bp.create {
                    Ok(bp)
                } else {
                    let e = format_err!(
                        "Unknown bookmark: {}. Use --create to create one.",
                        bp.name
                    );
                    Err(e)
                }
            })
            // Finally,, check that the push is a fast-forward, or --force is set.
            .and_then(|bp| match bp.force {
                true => ok(()).left_future(),
                false => check_is_ancestor_opt(ctx, repo, lca_hint, bp.old, bp.new)
                    .map_err(|e| format_err!("{} (try --force?)", e))
                    .right_future(),
            }.map(|_| bp))
            .map(Some)
            .left_future(),
        None => {
            warn!(ctx.logger(), "Infinitepush bookmark push to {} was ignored", bp.name; o!("remote" => "true"));
            ok(None)
        }.right_future()
    }
    .context("While verifying Infinite Push bookmark push")
    .from_err()
}

fn add_bookmark_to_transaction(
    txn: &mut Box<dyn Transaction>,
    bookmark_push: BookmarkPush<ChangesetId>,
    reason: BookmarkUpdateReason,
) -> Result<()> {
    match bookmark_push {
        BookmarkPush::PlainPush(PlainBookmarkPush { new, old, name, .. }) => match (new, old) {
            (Some(new), Some(old)) => txn.update(&name, new, old, reason),
            (Some(new), None) => txn.create(&name, new, reason),
            (None, Some(old)) => txn.delete(&name, old, reason),
            _ => Ok(()),
        },
        BookmarkPush::Infinitepush(InfiniteBookmarkPush { name, new, old, .. }) => match (new, old)
        {
            (new, Some(old)) => txn.update_infinitepush(&name, new, old),
            (new, None) => txn.create_infinitepush(&name, new),
        },
    }
}

fn get_optional_ascii_param(
    params: &HashMap<String, Bytes>,
    param: &str,
) -> Option<Result<AsciiString>> {
    params.get(param).map(|val| {
        AsciiString::from_ascii(val.to_vec())
            .map_err(|err| format_err!("`{}` parameter is not ascii: {}", param, err))
    })
}

fn get_ascii_param(params: &HashMap<String, Bytes>, param: &str) -> Result<AsciiString> {
    get_optional_ascii_param(params, param)
        .unwrap_or(Err(format_err!("`{}` parameter is not set", param)))
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

fn build_changegroup_push(
    ctx: CoreContext,
    repo: &BlobRepo,
    part_header: PartHeader,
    changesets: Changesets,
    filelogs: Filelogs,
    content_blobs: ContentBlobs,
) -> impl Future<Item = ChangegroupPush, Error = Error> {
    let PartHeaderInner {
        part_id,
        part_type,
        aparams,
        mparams,
        ..
    } = part_header.into_inner();

    let infinitepush_payload = match part_type {
        PartHeaderType::B2xInfinitepush => {
            let maybe_name_res = get_optional_ascii_param(&aparams, "bookmark")
                .transpose()
                .map(|maybe_name| maybe_name.map(BookmarkName::new_ascii));

            match maybe_name_res {
                Err(e) => err(e).left_future(),
                Ok(maybe_name) => match maybe_name {
                    None => ok(None).left_future(),
                    Some(name) => repo
                        .get_bookmark(ctx, &name)
                        .and_then(move |old| {
                            // NOTE: We do not validate that the bookmarknode selected (i.e. the
                            // changeset we should update our bookmark to) is part of the
                            // changegroup being pushed. We do however validate at a later point
                            // that this changeset exists.
                            let new = get_ascii_param(&aparams, "bookmarknode")?;
                            let new = HgChangesetId::from_ascii_str(&new)?;
                            let create = aparams.get("create").is_some();
                            let force = aparams.get("force").is_some();

                            Ok(InfiniteBookmarkPush {
                                name,
                                create,
                                force,
                                old,
                                new,
                            })
                        })
                        .map(Some)
                        .right_future(),
                }
                .right_future(),
            }
            .map(|bookmark_push| Some(InfinitepushPayload { bookmark_push }))
            .left_future()
        }
        _ => ok(None).right_future(),
    };

    infinitepush_payload.map(move |infinitepush_payload| ChangegroupPush {
        part_id,
        changesets,
        filelogs,
        content_blobs,
        mparams,
        infinitepush_payload,
    })
}

fn collect_pushkey_bookmark_pushes(
    pushkeys: Vec<Pushkey>,
) -> Vec<PlainBookmarkPush<HgChangesetId>> {
    pushkeys
        .into_iter()
        .filter_map(|pushkey| match pushkey {
            Pushkey::Phases => None,
            Pushkey::HgBookmarkPush(bp) => Some(bp),
        })
        .collect()
}

fn collect_all_bookmark_pushes(
    pushkeys: Vec<Pushkey>,
    infinitepush_bookmark_push: Option<InfiniteBookmarkPush<HgChangesetId>>,
) -> Vec<BookmarkPush<HgChangesetId>> {
    let mut bookmark_pushes: Vec<_> = collect_pushkey_bookmark_pushes(pushkeys)
        .into_iter()
        .map(BookmarkPush::PlainPush)
        .collect();

    if let Some(infinitepush_bookmark_push) = infinitepush_bookmark_push {
        bookmark_pushes.push(BookmarkPush::Infinitepush(infinitepush_bookmark_push));
    }

    bookmark_pushes
}

/// Helper fn to return some (usually "empty") value and
/// chain together an unused part with the rest of the bundle
fn return_with_rest_of_bundle<T: Send + 'static>(
    value: T,
    unused_part: Bundle2Item,
    rest_of_bundle: BoxStream<Bundle2Item, Error>,
) -> BoxFuture<(T, BoxStream<Bundle2Item, Error>), Error> {
    ok((
        value,
        stream::once(Ok(unused_part)).chain(rest_of_bundle).boxify(),
    ))
    .boxify()
}

fn toposort_changesets(
    changesets: Vec<(HgChangesetId, RevlogChangeset)>,
) -> Result<Vec<(HgChangesetId, RevlogChangeset)>> {
    let mut changesets: HashMap<_, _> = changesets.into_iter().collect();

    // Make sure changesets are toposorted
    let cs_id_to_parents: HashMap<_, _> = changesets
        .iter()
        .map(|(cs_id, revlog_cs)| {
            let parents: Vec<_> = revlog_cs
                .parents()
                .into_iter()
                .map(HgChangesetId::new)
                .collect();
            (*cs_id, parents)
        })
        .collect();
    let sorted_css =
        sort_topological(&cs_id_to_parents).ok_or(err_msg("cycle in the pushed changesets!"))?;

    Ok(sorted_css
        .into_iter()
        .rev() // reversing to get parents before the children
        .filter_map(|cs| changesets.remove(&cs).map(|revlog_cs| (cs, revlog_cs)))
        .collect())
}
