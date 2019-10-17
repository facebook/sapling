/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::changegroup::{
    convert_to_revlog_changesets, convert_to_revlog_filelog, split_changegroup,
};
use crate::errors::*;
use crate::stats::*;
use crate::upload_blobs::{upload_hg_blobs, UploadBlobsType, UploadableHgBlob};
use crate::upload_changesets::upload_changeset;
use ascii::AsciiString;
use blobrepo::{BlobRepo, ChangesetHandle};
use bookmarks::BookmarkName;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use core::fmt::{Debug, Display};
use failure::{err_msg, format_err, Compat, Context};
use failure_ext::{ensure_msg, FutureFailureErrorExt};
use futures::future::{self, err, ok, Shared};
use futures::stream;
use futures::{Future, IntoFuture, Stream};
use futures_ext::{try_boxfuture, BoxFuture, BoxStream, FutureExt, StreamExt};
use hooks::{ChangesetHookExecutionID, FileHookExecutionID, HookExecution};
use lazy_static::lazy_static;
use mercurial_bundles::{Bundle2Item, PartHeader, PartHeaderInner, PartHeaderType, PartId};
use mercurial_revlog::changeset::RevlogChangeset;
use mercurial_types::{
    blobs::{ContentBlobInfo, HgBlobEntry},
    HgChangesetId, HgNodeKey, RepoPath,
};
use metaconfig_types::RepoReadOnly;
use mononoke_types::{BlobstoreValue, BonsaiChangeset, RawBundle2, RawBundle2Id};
use scuba_ext::ScubaSampleBuilderExt;
use slog::{debug, trace};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use topo_sort::sort_topological;
use wirepack::{TreemanifestBundle2Parser, TreemanifestEntry};

pub type Changesets = Vec<(HgChangesetId, RevlogChangeset)>;
type Filelogs = HashMap<HgNodeKey, Shared<BoxFuture<(HgBlobEntry, RepoPath), Compat<Error>>>>;
type ContentBlobs = HashMap<HgNodeKey, ContentBlobInfo>;
type Manifests = HashMap<HgNodeKey, <TreemanifestEntry as UploadableHgBlob>::Value>;
type UploadedHgBonsaiMap = HashMap<HgChangesetId, BonsaiChangeset>;

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

impl<D: Display + Send + Sync + 'static> From<Context<D>> for BundleResolverError {
    fn from(context: Context<D>) -> Self {
        Self::Error(context.into())
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

/// Data, needed to perform post-resolve `Push` action
pub struct PostResolvePush {
    pub changegroup_id: Option<PartId>,
    pub bookmark_pushes: Vec<PlainBookmarkPush<HgChangesetId>>,
    pub maybe_raw_bundle2_id: Option<RawBundle2Id>,
    pub allow_non_fast_forward: bool,
}

/// Data, needed to perform post-resolve `InfinitePush` action
pub struct PostResolveInfinitePush {
    pub changegroup_id: Option<PartId>,
    pub bookmark_push: InfiniteBookmarkPush<HgChangesetId>,
    pub maybe_raw_bundle2_id: Option<RawBundle2Id>,
    pub allow_non_fast_forward: bool,
}

/// Data, needed to perform post-resolve `PushRebase` action
pub struct PostResolvePushRebase {
    pub changesets: Changesets,
    pub bookmark_push_part_id: Option<PartId>,
    pub bookmark_spec: PushrebaseBookmarkSpec,
    pub maybe_raw_bundle2_id: Option<RawBundle2Id>,
    pub maybe_pushvars: Option<HashMap<String, Bytes>>,
    pub commonheads: CommonHeads,
}

/// Data, needed to perform post-resolve `BookmarkOnlyPushRebase` action
pub struct PostResolveBookmarkOnlyPushRebase {
    pub bookmark_push: PlainBookmarkPush<HgChangesetId>,
    pub maybe_raw_bundle2_id: Option<RawBundle2Id>,
    pub allow_non_fast_forward: bool,
}

/// An action to take after the `unbundle` bundle2 was completely resolved
/// "Completely resolved" here means:
/// - parsed
/// - all received changesets and blobs uploaded
pub enum PostResolveAction {
    Push(PostResolvePush),
    InfinitePush(PostResolveInfinitePush),
    PushRebase(PostResolvePushRebase),
    BookmarkOnlyPushRebase(PostResolveBookmarkOnlyPushRebase),
}

/// The resolve function takes a bundle2, interprets it's content as Changesets, Filelogs and
/// Manifests and uploades all of them to the provided BlobRepo in the correct order.
/// It returns a Future that contains the response that should be send back to the requester.
pub fn resolve(
    ctx: CoreContext,
    repo: BlobRepo,
    infinitepush_writes_allowed: bool,
    bundle2: BoxStream<Bundle2Item, Error>,
    readonly: RepoReadOnly,
    maybe_full_content: Option<Arc<Mutex<Bytes>>>,
    pure_push_allowed: bool,
) -> BoxFuture<PostResolveAction, BundleResolverError> {
    let resolver = Bundle2Resolver::new(ctx.clone(), repo, infinitepush_writes_allowed);
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
    changegroup_acceptable: impl FnOnce() -> bool + Send + Sync + 'static,
) -> BoxFuture<PostResolveAction, Error> {
    resolver
        .maybe_resolve_changegroup(ctx.clone(), bundle2, changegroup_acceptable)
        .and_then({
            cloned!(resolver);
            move |(cg_push, bundle2)| {
                resolver
                    .resolve_multiple_parts(bundle2, Bundle2Resolver::maybe_resolve_pushkey)
                    .and_then(move |(pushkeys, bundle2)| {
                        let infinitepush_bp = cg_push
                            .as_ref()
                            .and_then(|cg_push| cg_push.infinitepush_payload.clone())
                            .and_then(|ip_payload| ip_payload.bookmark_push);
                        let bookmark_push =
                            try_collect_all_bookmark_pushes(pushkeys, infinitepush_bp)?;
                        Ok((cg_push, bookmark_push, bundle2))
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
                        .map(move |uploaded_map| {
                            (changegroup_id, bookmark_push, bundle2, Some(uploaded_map))
                        })
                        .boxify()
                } else {
                    ok((None, bookmark_push, bundle2, None)).boxify()
                }
            }
        })
        .and_then({
            cloned!(resolver);
            move |(changegroup_id, bookmark_push, bundle2, maybe_uploaded_map)| {
                resolver
                    .maybe_resolve_infinitepush_bookmarks(bundle2)
                    .map(move |((), bundle2)| {
                        (changegroup_id, bookmark_push, bundle2, maybe_uploaded_map)
                    })
            }
        })
        .and_then({
            cloned!(resolver);
            move |(changegroup_id, bookmark_push, bundle2, maybe_uploaded_map)| {
                resolver
                    .ensure_stream_finished(bundle2, maybe_full_content)
                    .map(move |maybe_raw_bundle2_id| {
                        (
                            changegroup_id,
                            bookmark_push,
                            maybe_raw_bundle2_id,
                            maybe_uploaded_map,
                        )
                    })
            }
        })
        .map({
            move |(
                changegroup_id,
                bookmark_push,
                maybe_raw_bundle2_id,
                _maybe_uploaded_hg_bonsai_map,
            )| {
                match bookmark_push {
                    AllBookmarkPushes::PlainPushes(bookmark_pushes) => {
                        PostResolveAction::Push(PostResolvePush {
                            changegroup_id,
                            bookmark_pushes,
                            maybe_raw_bundle2_id,
                            allow_non_fast_forward,
                        })
                    }
                    AllBookmarkPushes::Inifinitepush(bookmark_push) => {
                        PostResolveAction::InfinitePush(PostResolveInfinitePush {
                            changegroup_id,
                            bookmark_push,
                            maybe_raw_bundle2_id,
                            allow_non_fast_forward,
                        })
                    }
                }
            }
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
pub enum PushrebaseBookmarkSpec {
    NormalPushrebase(pushrebase::OntoBookmarkParams),
    ForcePushrebase(PlainBookmarkPush<HgChangesetId>),
}

impl PushrebaseBookmarkSpec {
    pub fn get_bookmark_name(&self) -> BookmarkName {
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
    maybe_full_content: Option<Arc<Mutex<Bytes>>>,
    changegroup_acceptable: impl FnOnce() -> bool + Send + Sync + 'static,
) -> BoxFuture<PostResolveAction, BundleResolverError> {
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
                    .map(move |_| (changesets, onto_params, bundle2)).right_future()
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
        .map({
            move |(changesets, bookmark_push_part_id, bookmark_spec, maybe_raw_bundle2_id)| {
                PostResolveAction::PushRebase(PostResolvePushRebase {
                    changesets,
                    bookmark_push_part_id,
                    bookmark_spec,
                    maybe_raw_bundle2_id,
                    maybe_pushvars,
                    commonheads,
                })
            }
        })
        .from_err()
        .boxify()
}

/// Do the right thing when pushrebase-enabled client only wants to manipulate bookmarks
fn resolve_bookmark_only_pushrebase(
    _ctx: CoreContext,
    resolver: Bundle2Resolver,
    bundle2: BoxStream<Bundle2Item, Error>,
    allow_non_fast_forward: bool,
    maybe_full_content: Option<Arc<Mutex<Bytes>>>,
) -> BoxFuture<PostResolveAction, Error> {
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
        .map({
            move |(bookmark_push, maybe_raw_bundle2_id)| {
                PostResolveAction::BookmarkOnlyPushRebase(PostResolveBookmarkOnlyPushRebase {
                    bookmark_push,
                    maybe_raw_bundle2_id,
                    allow_non_fast_forward,
                })
            }
        })
        .boxify()
}

fn next_item(
    bundle2: BoxStream<Bundle2Item, Error>,
) -> BoxFuture<(Option<Bundle2Item>, BoxStream<Bundle2Item, Error>), Error> {
    bundle2.into_future().map_err(|(err, _)| err).boxify()
}

/// Represents all the bookmark pushes that are created
/// by a single unbundle wireproto command. This can
/// be either an exactly one infinitepush, or multiple
/// plain pushes
pub enum AllBookmarkPushes<T: Copy> {
    PlainPushes(Vec<PlainBookmarkPush<T>>),
    Inifinitepush(InfiniteBookmarkPush<T>),
}

/// Represets a single non-infinitepush bookmark push
/// This can be a result of a normal push or a pushrebase
#[derive(Debug, Clone)]
pub struct PlainBookmarkPush<T: Copy> {
    pub part_id: PartId,
    pub name: BookmarkName,
    pub old: Option<T>,
    pub new: Option<T>,
}

/// Represents an infinitepush bookmark push
#[derive(Debug, Clone)]
pub struct InfiniteBookmarkPush<T> {
    pub name: BookmarkName,
    pub create: bool,
    pub force: bool,
    pub old: Option<T>,
    pub new: T,
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

pub struct CommonHeads {
    pub heads: Vec<HgChangesetId>,
}

enum Pushkey {
    HgBookmarkPush(PlainBookmarkPush<HgChangesetId>),
    Phases,
}

/// Holds repo and logger for convienience access from it's methods
#[derive(Clone)]
pub struct Bundle2Resolver {
    ctx: CoreContext,
    repo: BlobRepo,
    infinitepush_writes_allowed: bool,
}

impl Bundle2Resolver {
    fn new(ctx: CoreContext, repo: BlobRepo, infinitepush_writes_allowed: bool) -> Self {
        Self {
            ctx,
            repo,
            infinitepush_writes_allowed,
        }
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
        let fut = if self.infinitepush_writes_allowed {
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
    ) -> BoxFuture<UploadedHgBonsaiMap, Error> {
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
        let res: BoxFuture<UploadedHgBonsaiMap, Error> = stream::iter_ok::<_, Error>(changesets)
            .chunks(chunk_size)
            .fold(UploadedHgBonsaiMap::new(), move |mut mapping, chunk| {
                stream::iter_ok(chunk)
                    .fold(HashMap::new(), {
                        cloned!(upload_changeset_fun);
                        move |uploaded_changesets, (node, revlog_cs)| {
                            (*upload_changeset_fun)(uploaded_changesets, node, revlog_cs)
                        }
                    })
                    .and_then({
                        move |uploaded_changesets| {
                            stream::iter_ok(uploaded_changesets.into_iter().map(
                                move |(hg_cs_id, handle)| {
                                    handle.get_completed_changeset().map(move |shared_item| {
                                        let bcs = shared_item.0.clone();
                                        (hg_cs_id, bcs)
                                    })
                                },
                            ))
                            .buffered(chunk_size)
                            .map_err(Error::from)
                            .collect()
                        }
                    })
                    .map(move |uploaded| {
                        mapping.extend(uploaded.into_iter());
                        mapping
                    })
                    .boxify()
            })
            .chain_err(ErrorKind::WhileUploadingData(changesets_hashes))
            .from_err()
            .boxify();
        res
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

fn try_collect_all_bookmark_pushes(
    pushkeys: Vec<Pushkey>,
    infinitepush_bookmark_push: Option<InfiniteBookmarkPush<HgChangesetId>>,
) -> Result<AllBookmarkPushes<HgChangesetId>> {
    let bookmark_pushes: Vec<_> = collect_pushkey_bookmark_pushes(pushkeys)
        .into_iter()
        .collect();
    let bookmark_pushes_len = bookmark_pushes.len();
    match (bookmark_pushes_len, infinitepush_bookmark_push) {
        (0, Some(infinitepush_bookmark_push)) => {
            STATS::bookmark_pushkeys_count.add_value(1);
            Ok(AllBookmarkPushes::Inifinitepush(infinitepush_bookmark_push))
        }
        (bookmark_pushes_len, None) => {
            STATS::bookmark_pushkeys_count.add_value(bookmark_pushes_len as i64);
            Ok(AllBookmarkPushes::PlainPushes(bookmark_pushes))
        }
        (_, Some(_)) => Err(format_err!(
            "Same bundle2 can not be used for both plain and infinite push"
        )),
    }
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
