/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::changegroup::{
    convert_to_revlog_changesets, convert_to_revlog_filelog, split_changegroup,
};
use crate::errors::*;
use crate::stats::*;
use crate::upload_blobs::{upload_hg_blobs, UploadBlobsType, UploadableHgBlob};
use crate::upload_changesets::upload_changeset;
use anyhow::{bail, ensure, format_err, Context, Error, Result};
use ascii::AsciiString;
use blobrepo::{BlobRepo, ChangesetHandle};
use blobstore::Storable;
use bookmarks::BookmarkName;
use bytes::Bytes;
use bytes_old::Bytes as BytesOld;
use context::CoreContext;
use core::fmt::Debug;
use failure_ext::{Compat, FutureFailureErrorExt};
use futures::future::{try_join_all, Future};
use futures::stream;
use futures_ext::{
    BoxFuture as OldBoxFuture, BoxStream as OldBoxStream, FutureExt as OldFutureExt,
    StreamExt as OldStreamExt,
};
use futures_old::future::{err, Shared};
use futures_old::stream as old_stream;
use futures_old::{Future as OldFuture, Stream as OldStream};
use futures_util::{
    compat::Future01CompatExt, try_join, FutureExt, StreamExt, TryFutureExt, TryStreamExt,
};
use hooks::HookRejectionInfo;
use lazy_static::lazy_static;
use limits::types::RateLimit;
use mercurial_bundles::{Bundle2Item, PartHeader, PartHeaderInner, PartHeaderType, PartId};
use mercurial_revlog::changeset::RevlogChangeset;
use mercurial_types::{
    blobs::{ContentBlobInfo, HgBlobEntry},
    HgChangesetId, HgNodeKey, RepoPath,
};
use metaconfig_types::{PushrebaseFlags, RepoReadOnly};
use mononoke_types::{BlobstoreValue, BonsaiChangeset, ChangesetId, RawBundle2, RawBundle2Id};
use pushrebase::HgReplayData;
use scuba_ext::ScubaSampleBuilderExt;
use slog::{debug, trace};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use topo_sort::sort_topological;
use wirepack::{TreemanifestBundle2Parser, TreemanifestEntry};

#[allow(non_snake_case)]
mod UNBUNDLE_STATS {
    use stats::define_stats;

    define_stats! {
        prefix = "mononoke.unbundle.resolver";
        push: dynamic_timeseries("{}.push", (reponame: String); Rate, Sum),
        pushrebase: dynamic_timeseries("{}.pushrebase", (reponame: String); Rate, Sum),
        bookmark_only_pushrebase: dynamic_timeseries("{}.bookmark_only_pushrebase", (reponame: String); Rate, Sum),
        infinitepush: dynamic_timeseries("{}.infinitepush", (reponame: String); Rate, Sum),
        resolver_error: dynamic_timeseries("{}.resolver_error", (reponame: String); Rate, Sum),
    }

    pub use self::STATS::*;
}

pub type Changesets = Vec<(HgChangesetId, RevlogChangeset)>;
type Filelogs = HashMap<HgNodeKey, Shared<OldBoxFuture<(HgBlobEntry, RepoPath), Compat<Error>>>>;
type ContentBlobs = HashMap<HgNodeKey, ContentBlobInfo>;
type Manifests = HashMap<HgNodeKey, <TreemanifestEntry as UploadableHgBlob>::Value>;
pub type UploadedBonsais = HashSet<BonsaiChangeset>;

// This is to match the core hg behavior from https://fburl.com/jf3iyl7y
// Mercurial substitutes the `onto` parameter with this bookmark name when
// the force pushrebase is done, so we need to look for it and make sure we
// do the right thing here too.
lazy_static! {
    static ref DONOTREBASEBOOKMARK: BookmarkName =
        BookmarkName::new("__pushrebase_donotrebase__").unwrap();
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NonFastForwardPolicy {
    Allowed,
    Disallowed,
}

impl From<bool> for NonFastForwardPolicy {
    fn from(allowed: bool) -> Self {
        if allowed {
            Self::Allowed
        } else {
            Self::Disallowed
        }
    }
}

pub struct HookFailure {
    pub(crate) hook_name: String,
    pub(crate) cs_id: HgChangesetId,
    pub(crate) info: HookRejectionInfo,
}

impl HookFailure {
    pub fn get_hook_name(&self) -> &str {
        &self.hook_name
    }
}

pub enum BundleResolverError {
    HookError(Vec<HookFailure>),
    PushrebaseConflicts(Vec<pushrebase::PushrebaseConflict>),
    Error(Error),
    RateLimitExceeded {
        limit: RateLimit,
        entity: String,
        value: f64,
    },
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
            HookError(hook_outcomes) => {
                let err_msgs: Vec<_> = hook_outcomes
                    .into_iter()
                    .map(|failure| {
                        format!(
                            "{} for {}: {}",
                            failure.hook_name, failure.cs_id, failure.info.long_description
                        )
                    })
                    .collect();
                format_err!("hooks failed:\n{}", err_msgs.join("\n"))
            }
            PushrebaseConflicts(conflicts) => {
                format_err!("pushrebase failed Conflicts({:?})", conflicts)
            }
            RateLimitExceeded {
                limit,
                entity,
                value,
            } => format_err!(
                "Rate limit exceeded: {} for {}. \
                 The maximum allowed value is {} over a sliding {}s interval. \
                 The observed value was {}. For help: {}.",
                limit.name,
                entity,
                limit.max_value,
                limit.interval,
                value,
                limit.help,
            ),
            Error(err) => err,
        }
    }
}

/// Data, needed to perform post-resolve `Push` action
pub struct PostResolvePush {
    pub changegroup_id: Option<PartId>,
    pub bookmark_pushes: Vec<PlainBookmarkPush<ChangesetId>>,
    pub maybe_raw_bundle2_id: Option<RawBundle2Id>,
    pub non_fast_forward_policy: NonFastForwardPolicy,
    pub uploaded_bonsais: UploadedBonsais,
}

/// Data, needed to perform post-resolve `InfinitePush` action
pub struct PostResolveInfinitePush {
    pub changegroup_id: Option<PartId>,
    pub bookmark_push: InfiniteBookmarkPush<ChangesetId>,
    pub maybe_raw_bundle2_id: Option<RawBundle2Id>,
    pub uploaded_bonsais: UploadedBonsais,
}

/// Data, needed to perform post-resolve `PushRebase` action
pub struct PostResolvePushRebase {
    pub any_merges: bool,
    pub bookmark_push_part_id: Option<PartId>,
    pub bookmark_spec: PushrebaseBookmarkSpec<ChangesetId>,
    pub maybe_hg_replay_data: Option<HgReplayData>,
    pub maybe_pushvars: Option<HashMap<String, Bytes>>,
    pub commonheads: CommonHeads,
    pub uploaded_bonsais: UploadedBonsais,
}

/// Data, needed to perform post-resolve `BookmarkOnlyPushRebase` action
pub struct PostResolveBookmarkOnlyPushRebase {
    pub bookmark_push: PlainBookmarkPush<ChangesetId>,
    pub maybe_raw_bundle2_id: Option<RawBundle2Id>,
    pub non_fast_forward_policy: NonFastForwardPolicy,
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
pub async fn resolve<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    infinitepush_writes_allowed: bool,
    bundle2: OldBoxStream<Bundle2Item, Error>,
    readonly: RepoReadOnly,
    maybe_full_content: Option<Arc<Mutex<BytesOld>>>,
    pure_push_allowed: bool,
    pushrebase_flags: PushrebaseFlags,
) -> Result<PostResolveAction, BundleResolverError> {
    let resolver = Bundle2Resolver::new(ctx, repo, infinitepush_writes_allowed, pushrebase_flags);
    let bundle2 = resolver.resolve_start_and_replycaps(bundle2);

    let (maybe_commonheads, bundle2) = resolver.maybe_resolve_commonheads(bundle2).await?;
    let (maybe_pushvars, bundle2) = resolver
        .maybe_resolve_pushvars(bundle2)
        .await
        .context("While resolving Pushvars")?;

    let mut bypass_readonly = false;
    // check the bypass condition
    if let Some(ref pushvars) = maybe_pushvars {
        bypass_readonly = pushvars
            .get("BYPASS_READONLY")
            .map(|s| s.to_ascii_lowercase())
            == Some("true".into());
    }

    if let RepoReadOnly::ReadOnly(reason) = readonly {
        if bypass_readonly == false {
            let e = Error::from(ErrorKind::RepoReadOnly(reason));
            return Err(e.into());
        }
    }

    let (pushkey_next, bundle2) = resolver.is_next_part_pushkey(bundle2).await?;

    let non_fast_forward_policy = {
        let mut allow_non_fast_forward = false;
        // check the bypass condition
        if let Some(ref pushvars) = maybe_pushvars {
            allow_non_fast_forward = pushvars
                .get("NON_FAST_FORWARD")
                .map(|s| s.to_ascii_lowercase())
                == Some("true".into());
        }
        NonFastForwardPolicy::from(allow_non_fast_forward)
    };

    let post_resolve_action = if let Some(commonheads) = maybe_commonheads {
        if pushkey_next {
            resolve_bookmark_only_pushrebase(
                ctx,
                resolver,
                bundle2,
                non_fast_forward_policy,
                maybe_full_content,
            )
            .await
            .map_err(BundleResolverError::from)
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
            .await
        }
    } else {
        resolve_push(
            ctx,
            resolver,
            bundle2,
            non_fast_forward_policy,
            maybe_full_content,
            move || pure_push_allowed,
        )
        .await
        .context("bundle2_resolver error")
        .map_err(BundleResolverError::from)
    };

    report_unbundle_type(ctx, repo, &post_resolve_action);
    post_resolve_action
}

fn report_unbundle_type(
    ctx: &CoreContext,
    repo: &BlobRepo,
    post_resolve_action: &Result<PostResolveAction, BundleResolverError>,
) {
    let repo_name = repo.name().clone();
    match post_resolve_action {
        Ok(PostResolveAction::Push(_)) => {
            ctx.scuba()
                .clone()
                .log_with_msg("Unbundle resolved", Some("push".to_owned()));
            UNBUNDLE_STATS::push.add_value(1, (repo_name,))
        }
        Ok(PostResolveAction::PushRebase(_)) => {
            ctx.scuba()
                .clone()
                .log_with_msg("Unbundle resolved", Some("pushrebase".to_owned()));
            UNBUNDLE_STATS::pushrebase.add_value(1, (repo_name,))
        }
        Ok(PostResolveAction::InfinitePush(_)) => {
            ctx.scuba()
                .clone()
                .log_with_msg("Unbunble resolved", Some("infinitepush".to_owned()));
            UNBUNDLE_STATS::infinitepush.add_value(1, (repo_name,))
        }
        Ok(PostResolveAction::BookmarkOnlyPushRebase(_)) => {
            ctx.scuba().clone().log_with_msg(
                "Unbundle resolved",
                Some("bookmark_only_pushrebase".to_owned()),
            );
            UNBUNDLE_STATS::bookmark_only_pushrebase.add_value(1, (repo_name,))
        }
        Err(_) => UNBUNDLE_STATS::resolver_error.add_value(1, (repo_name,)),
    }
}

async fn resolve_push<'r>(
    ctx: &'r CoreContext,
    resolver: Bundle2Resolver<'r>,
    bundle2: OldBoxStream<Bundle2Item, Error>,
    non_fast_forward_policy: NonFastForwardPolicy,
    maybe_full_content: Option<Arc<Mutex<BytesOld>>>,
    changegroup_acceptable: impl FnOnce() -> bool + Send + Sync + 'static,
) -> Result<PostResolveAction, Error> {
    let (cg_push, bundle2) = resolver
        .maybe_resolve_changegroup(bundle2, changegroup_acceptable)
        .await
        .context("While resolving Changegroup")?;
    let (pushkeys, bundle2) = resolver
        .resolve_multiple_parts(bundle2, Bundle2Resolver::maybe_resolve_pushkey)
        .await
        .context("While resolving Pushkey")?;
    let infinitepush_bp = cg_push
        .as_ref()
        .and_then(|cg_push| cg_push.infinitepush_payload.as_ref())
        .and_then(|ip_payload| ip_payload.bookmark_push.as_ref());
    let bookmark_push = try_collect_all_bookmark_pushes(pushkeys, infinitepush_bp.cloned())?;

    let (cg_and_manifests, bundle2) = if let Some(cg_push) = cg_push {
        let (manifests, bundle2) = resolver
            .resolve_b2xtreegroup2(bundle2)
            .await
            .context("While resolving B2xTreegroup2")?;
        (Some((cg_push, manifests)), bundle2)
    } else {
        (None, bundle2)
    };

    let (changegroup_id, uploaded_bonsais) = if let Some((cg_push, manifests)) = cg_and_manifests {
        let changegroup_id = Some(cg_push.part_id);
        let uploaded_bonsais = resolver.upload_changesets(cg_push, manifests).await?;

        // Note: we do not care about `_uploaded_hg_changesets`, as we currently
        // do not run hooks on pure pushes. This probably has to be changed later.
        (changegroup_id, uploaded_bonsais)
    } else {
        (None, UploadedBonsais::new())
    };

    let ((), bundle2) = resolver
        .maybe_resolve_infinitepush_bookmarks(bundle2)
        .await
        .context("While resolving B2xInfinitepushBookmarks")?;
    let maybe_raw_bundle2_id = resolver
        .ensure_stream_finished(bundle2, maybe_full_content)
        .await?;
    let bookmark_push =
        hg_all_bookmark_pushes_to_bonsai(ctx, &resolver.repo, bookmark_push).await?;

    Ok(match bookmark_push {
        AllBookmarkPushes::PlainPushes(bookmark_pushes) => {
            PostResolveAction::Push(PostResolvePush {
                changegroup_id,
                bookmark_pushes,
                maybe_raw_bundle2_id,
                non_fast_forward_policy,
                uploaded_bonsais,
            })
        }
        AllBookmarkPushes::Inifinitepush(bookmark_push) => {
            PostResolveAction::InfinitePush(PostResolveInfinitePush {
                changegroup_id,
                bookmark_push,
                maybe_raw_bundle2_id,
                uploaded_bonsais,
            })
        }
    })
}

// Enum used to pass data for normal or forceful pushrebases
// Normal pushrebase is what one would expect: take a (potentially
// stack of) commit(s) and rebase it on top of a given bookmark.
// Force pushrebase is basically a push, which for logging
// and respondin purposes is treated like a pushrebase
pub enum PushrebaseBookmarkSpec<T: Copy> {
    NormalPushrebase(pushrebase::OntoBookmarkParams),
    ForcePushrebase(PlainBookmarkPush<T>),
}

impl<T: Copy> PushrebaseBookmarkSpec<T> {
    pub fn get_bookmark_name(&self) -> &BookmarkName {
        match self {
            PushrebaseBookmarkSpec::NormalPushrebase(onto_params) => &onto_params.bookmark,
            PushrebaseBookmarkSpec::ForcePushrebase(plain_push) => &plain_push.name,
        }
    }
}

async fn resolve_pushrebase<'r>(
    ctx: &'r CoreContext,
    commonheads: CommonHeads,
    resolver: Bundle2Resolver<'r>,
    bundle2: OldBoxStream<Bundle2Item, Error>,
    maybe_pushvars: Option<HashMap<String, Bytes>>,
    maybe_full_content: Option<Arc<Mutex<BytesOld>>>,
    changegroup_acceptable: impl FnOnce() -> bool + Send + Sync + 'static,
) -> Result<PostResolveAction, BundleResolverError> {
    let (manifests, bundle2) = resolver
        .resolve_b2xtreegroup2(bundle2)
        .await
        .context("While resolving B2xTreegroup2")?;
    let (maybe_cg_push, bundle2) = resolver
        .maybe_resolve_changegroup(bundle2, changegroup_acceptable)
        .await
        .context("While resolving Changegroup")?;
    let cg_push = maybe_cg_push.ok_or(Error::msg("Empty pushrebase"))?;
    let onto_params = match cg_push.mparams.get("onto") {
        Some(onto_bookmark) => {
            let v = Vec::from(onto_bookmark.as_ref());
            let onto_bookmark = String::from_utf8(v).map_err(Error::from)?;
            let onto_bookmark = BookmarkName::new(onto_bookmark)?;
            let onto_bookmark = pushrebase::OntoBookmarkParams::new(onto_bookmark);
            onto_bookmark
        }
        None => return Err(format_err!("onto is not specified").into()),
    };

    let changesets = &cg_push.changesets.clone();
    let any_merges = changesets
        .iter()
        .any(|(_, revlog_cs)| revlog_cs.p1.is_some() && revlog_cs.p2.is_some());

    let will_rebase = onto_params.bookmark != *DONOTREBASEBOOKMARK;
    // Mutation information must not be present in public commits
    // See T54101162, S186586
    if !will_rebase {
        for (_, hg_cs) in changesets {
            for key in pushrebase::MUTATION_KEYS {
                if hg_cs.extra.as_ref().contains_key(key.as_bytes()) {
                    return Err(Error::msg("Forced push blocked because it contains mutation metadata.\n\
                                You can remove the metadata from a commit with `hg amend --config mutation.record=false`.\n\
                                For more help, please contact the Source Control team at https://fburl.com/27qnuyl2").into());
                }
            }
        }
    }

    let uploaded_bonsais = resolver.upload_changesets(cg_push, manifests).await?;

    let (pushkeys, bundle2) = resolver
        .resolve_multiple_parts(bundle2, Bundle2Resolver::maybe_resolve_pushkey)
        .await
        .context("While resolving Pushkey")?;

    let bookmark_pushes = collect_pushkey_bookmark_pushes(pushkeys);
    if bookmark_pushes.len() > 1 {
        return Err(format_err!("Too many pushkey parts: {:?}", bookmark_pushes).into());
    }

    let (bookmark_push_part_id, bookmark_spec) = match bookmark_pushes.into_iter().next() {
        Some(bk_push)
            if bk_push.name != onto_params.bookmark
                && onto_params.bookmark != *DONOTREBASEBOOKMARK =>
        {
            return Err(format_err!(
                "allowed only pushes of {} bookmark: {:?}",
                onto_params.bookmark,
                bk_push
            )
            .into());
        }
        Some(bk_push) if onto_params.bookmark == *DONOTREBASEBOOKMARK => {
            (
                // This is a force pushrebase scenario. We need to ignore `onto_params`
                // and run normal push (using bk_push), but generate a pushrebase
                // response.
                // See comment next to DONOTREBASEBOOKMARK definition
                Some(bk_push.part_id),
                PushrebaseBookmarkSpec::ForcePushrebase(bk_push),
            )
        }
        Some(bk_push) => (
            Some(bk_push.part_id),
            PushrebaseBookmarkSpec::NormalPushrebase(onto_params),
        ),
        None => (None, PushrebaseBookmarkSpec::NormalPushrebase(onto_params)),
    };

    let maybe_raw_bundle2_id = resolver
        .ensure_stream_finished(bundle2, maybe_full_content)
        .await?;
    let bookmark_spec =
        hg_pushrebase_bookmark_spec_to_bonsai(ctx, &resolver.repo, bookmark_spec).await?;
    let repo = resolver.repo.clone();
    let maybe_hg_replay_data = maybe_raw_bundle2_id.map(|raw_bundle2_id| {
        HgReplayData::new_with_simple_convertor(ctx.clone(), raw_bundle2_id, repo)
    });

    Ok(PostResolveAction::PushRebase(PostResolvePushRebase {
        any_merges,
        bookmark_push_part_id,
        bookmark_spec,
        maybe_hg_replay_data,
        maybe_pushvars,
        commonheads,
        uploaded_bonsais,
    }))
}

/// Do the right thing when pushrebase-enabled client only wants to manipulate bookmarks
async fn resolve_bookmark_only_pushrebase<'r>(
    ctx: &'r CoreContext,
    resolver: Bundle2Resolver<'r>,
    bundle2: OldBoxStream<Bundle2Item, Error>,
    non_fast_forward_policy: NonFastForwardPolicy,
    maybe_full_content: Option<Arc<Mutex<BytesOld>>>,
) -> Result<PostResolveAction, Error> {
    // TODO: we probably run hooks even if no changesets are pushed?
    //       however, current run_hooks implementation will no-op such thing

    let (pushkeys, bundle2) = resolver
        .resolve_multiple_parts(bundle2, Bundle2Resolver::maybe_resolve_pushkey)
        .await
        .context("While resolving Pushkey")?;
    let pushkeys_len = pushkeys.len();
    let bookmark_pushes = collect_pushkey_bookmark_pushes(pushkeys);

    // this means we filtered some Phase pushkeys out
    // which is not expected
    if bookmark_pushes.len() != pushkeys_len {
        return Err(Error::msg(
            "Expected bookmark-only push, phases pushkey found",
        ));
    }

    if bookmark_pushes.len() != 1 {
        return Err(format_err!("Too many pushkey parts: {:?}", bookmark_pushes));
    }

    let bookmark_push = bookmark_pushes.into_iter().next().unwrap();
    let maybe_raw_bundle2_id = resolver
        .ensure_stream_finished(bundle2, maybe_full_content)
        .await?;
    let bookmark_push =
        plain_hg_bookmark_push_to_bonsai(ctx, &resolver.repo, bookmark_push).await?;

    Ok(PostResolveAction::BookmarkOnlyPushRebase(
        PostResolveBookmarkOnlyPushRebase {
            bookmark_push,
            maybe_raw_bundle2_id,
            non_fast_forward_policy,
        },
    ))
}

async fn next_item(
    bundle2: OldBoxStream<Bundle2Item, Error>,
) -> Result<(Option<Bundle2Item>, OldBoxStream<Bundle2Item, Error>), Error> {
    bundle2.into_future().map_err(|(err, _)| err).compat().await
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
pub struct Bundle2Resolver<'r> {
    ctx: &'r CoreContext,
    repo: &'r BlobRepo,
    infinitepush_writes_allowed: bool,
    pushrebase_flags: PushrebaseFlags,
}

impl<'r> Bundle2Resolver<'r> {
    fn new(
        ctx: &'r CoreContext,
        repo: &'r BlobRepo,
        infinitepush_writes_allowed: bool,
        pushrebase_flags: PushrebaseFlags,
    ) -> Self {
        Self {
            ctx,
            repo,
            infinitepush_writes_allowed,
            pushrebase_flags,
        }
    }

    /// Peek at the next `bundle2` item and check if it is a `Pushkey` part
    /// Return unchanged `bundle2`
    async fn is_next_part_pushkey(
        &self,
        bundle2: OldBoxStream<Bundle2Item, Error>,
    ) -> Result<(bool, OldBoxStream<Bundle2Item, Error>), Error> {
        let (start, bundle2) = next_item(bundle2).await?;
        match start {
            Some(part) => {
                if let Bundle2Item::Pushkey(header, box_future) = part {
                    Ok((
                        true,
                        old_stream::once(Ok(Bundle2Item::Pushkey(header, box_future)))
                            .chain(bundle2)
                            .boxify(),
                    ))
                } else {
                    return_with_rest_of_bundle(false, part, bundle2).await
                }
            }
            _ => Ok((false, bundle2)),
        }
    }

    /// Preserve the full raw content of the bundle2 for later replay
    async fn maybe_save_full_content_bundle2(
        &self,
        maybe_full_content: Option<Arc<Mutex<BytesOld>>>,
    ) -> Result<Option<RawBundle2Id>, Error> {
        match maybe_full_content {
            Some(full_content) => {
                let blob =
                    RawBundle2::new_bytes(Bytes::copy_from_slice(&full_content.lock().unwrap()))
                        .into_blob();
                let id = blob
                    .store(self.ctx.clone(), self.repo.blobstore())
                    .compat()
                    .await?;
                debug!(self.ctx.logger(), "Saved a raw bundle2 content: {:?}", id);
                self.ctx
                    .scuba()
                    .clone()
                    .log_with_msg("Saved a raw bundle2 content", Some(format!("{}", id)));
                Ok(Some(id))
            }
            None => Ok(None),
        }
    }

    /// Parse Start and Replycaps and ignore their content
    fn resolve_start_and_replycaps(
        &self,
        bundle2: OldBoxStream<Bundle2Item, Error>,
    ) -> OldBoxStream<Bundle2Item, Error> {
        next_item(bundle2)
            .boxed()
            .compat()
            .and_then(|(start, bundle2)| match start {
                Some(Bundle2Item::Start(_)) => next_item(bundle2).boxed().compat().left_future(),
                _ => err(format_err!("Expected Bundle2 Start")).right_future(),
            })
            .and_then(|(replycaps, bundle2)| match replycaps {
                Some(Bundle2Item::Replycaps(_, part)) => part.map(|_| bundle2).left_future(),
                _ => err(format_err!("Expected Bundle2 Replycaps")).right_future(),
            })
            .flatten_stream()
            .boxify()
    }

    // Parse b2x:commonheads
    // This part sent by pushrebase so that server can find out what commits to send back to the
    // client. This part is used as a marker that this push is pushrebase.
    async fn maybe_resolve_commonheads(
        &self,
        bundle2: OldBoxStream<Bundle2Item, Error>,
    ) -> Result<(Option<CommonHeads>, OldBoxStream<Bundle2Item, Error>), Error> {
        let (maybe_commonheads, bundle2) = next_item(bundle2).await?;

        match maybe_commonheads {
            Some(Bundle2Item::B2xCommonHeads(_header, heads)) => {
                let heads = heads.collect().compat().await?;
                let heads = CommonHeads { heads };
                Ok((Some(heads), bundle2))
            }

            Some(part) => return_with_rest_of_bundle(None, part, bundle2).await,
            _ => Err(format_err!("Unexpected Bundle2 stream end")),
        }
    }

    /// Parse pushvars
    /// It is used to store hook arguments.
    async fn maybe_resolve_pushvars(
        &self,
        bundle2: OldBoxStream<Bundle2Item, Error>,
    ) -> Result<
        (
            Option<HashMap<String, Bytes>>,
            OldBoxStream<Bundle2Item, Error>,
        ),
        Error,
    > {
        let (newpart, bundle2) = next_item(bundle2).await?;

        let maybe_pushvars = match newpart {
            Some(Bundle2Item::Pushvars(header, emptypart)) => {
                let pushvars = header.into_inner().aparams;
                // ignored for now, will be used for hooks
                emptypart.compat().await?;
                Some(pushvars)
            }
            Some(part) => return return_with_rest_of_bundle(None, part, bundle2).await,
            None => None,
        };

        Ok((maybe_pushvars, bundle2))
    }

    /// Parse changegroup.
    /// The ChangegroupId will be used in the last step for preparing response
    /// The Changesets should be parsed as RevlogChangesets and used for uploading changesets
    /// The Filelogs should be scheduled for uploading to BlobRepo and the Future resolving in
    /// their upload should be used for uploading changesets
    /// `pure_push_allowed` argument is responsible for allowing
    /// pure (non-pushrebase and non-infinitepush) pushes
    async fn maybe_resolve_changegroup(
        &self,
        bundle2: OldBoxStream<Bundle2Item, Error>,
        changegroup_acceptable: impl FnOnce() -> bool + Send + Sync + 'static,
    ) -> Result<(Option<ChangegroupPush>, OldBoxStream<Bundle2Item, Error>), Error> {
        let infinitepush_writes_allowed = self.infinitepush_writes_allowed;

        let (changegroup, bundle2) = next_item(bundle2).await?;

        let maybe_cg_push: Option<ChangegroupPush> = match changegroup {
            // XXX: we may be interested in checking that this is a correct changegroup part
            // type
            Some(Bundle2Item::Changegroup(header, parts))
            | Some(Bundle2Item::B2xInfinitepush(header, parts))
            | Some(Bundle2Item::B2xRebase(header, parts)) => {
                if header.part_type() == &PartHeaderType::Changegroup && !changegroup_acceptable() {
                    // Changegroup part type signals that we are in a pure push scenario
                    return Err(format_err!("Pure pushes are disallowed in this repo"));
                }

                let (changesets, filelogs) = split_changegroup(parts);
                let changesets = convert_to_revlog_changesets(changesets)
                    .collect()
                    .compat()
                    .await?;
                let upload_map = upload_hg_blobs(
                    self.ctx.clone(),
                    self.repo.clone(),
                    convert_to_revlog_filelog(self.ctx.clone(), self.repo.clone(), filelogs),
                    UploadBlobsType::EnsureNoDuplicates,
                )
                .compat()
                .await
                .context("While uploading File Blobs")?;

                let (filelogs, content_blobs) = {
                    let mut filelogs = HashMap::new();
                    let mut content_blobs = HashMap::new();
                    for (node_key, (cbinfo, file_upload)) in upload_map {
                        filelogs.insert(node_key.clone(), file_upload);
                        content_blobs.insert(node_key, cbinfo);
                    }
                    (filelogs, content_blobs)
                };

                let cg_push = build_changegroup_push(
                    &self.ctx,
                    &self.repo,
                    header,
                    changesets,
                    filelogs,
                    content_blobs,
                )
                .await?;

                Some(cg_push)
            }
            Some(part) => return return_with_rest_of_bundle(None, part, bundle2).await,
            _ => return Err(format_err!("Unexpected Bundle2 stream end")),
        };

        // Check that infinitepush is enabled if we use it.
        if infinitepush_writes_allowed {
            Ok((maybe_cg_push, bundle2))
        } else {
            match maybe_cg_push {
                Some(ref cg_push) if cg_push.infinitepush_payload.is_some() => {
                    bail!(
                        "Infinitepush is not enabled on this server. Contact Source Control @ FB."
                    );
                }
                other => Ok((other, bundle2)),
            }
        }
    }

    /// Parses pushkey part if it exists
    /// Returns an error if the pushkey namespace is unknown
    async fn maybe_resolve_pushkey(
        &self,
        bundle2: OldBoxStream<Bundle2Item, Error>,
    ) -> Result<(Option<Pushkey>, OldBoxStream<Bundle2Item, Error>), Error> {
        let (newpart, bundle2) = next_item(bundle2).await?;

        match newpart {
            Some(Bundle2Item::Pushkey(header, emptypart)) => {
                let namespace = header
                    .mparams()
                    .get("namespace")
                    .ok_or(format_err!("pushkey: `namespace` parameter is not set"))?;

                let pushkey = match &namespace[..] {
                    b"phases" => Pushkey::Phases,
                    b"bookmarks" => {
                        let part_id = header.part_id();
                        let mparams = header.mparams();
                        let name = get_ascii_param(mparams, "key")?;
                        let name = BookmarkName::new_ascii(name);
                        let old = get_optional_changeset_param(mparams, "old")?;
                        let new = get_optional_changeset_param(mparams, "new")?;

                        Pushkey::HgBookmarkPush(PlainBookmarkPush {
                            part_id,
                            name,
                            old,
                            new,
                        })
                    }
                    _ => {
                        return Err(format_err!(
                            "pushkey: unexpected namespace: {:?}",
                            namespace
                        ));
                    }
                };

                emptypart.compat().await?;
                Ok((Some(pushkey), bundle2))
            }
            Some(part) => return_with_rest_of_bundle(None, part, bundle2).await,
            None => Ok((None, bundle2)),
        }
    }

    /// Parse b2xtreegroup2.
    /// The Manifests should be scheduled for uploading to BlobRepo and the Future resolving in
    /// their upload as well as their parsed content should be used for uploading changesets.
    async fn resolve_b2xtreegroup2(
        &self,
        bundle2: OldBoxStream<Bundle2Item, Error>,
    ) -> Result<(Manifests, OldBoxStream<Bundle2Item, Error>), Error> {
        let (b2xtreegroup2, bundle2) = next_item(bundle2).await?;

        match b2xtreegroup2 {
            Some(Bundle2Item::B2xTreegroup2(_, parts))
            | Some(Bundle2Item::B2xRebasePack(_, parts)) => {
                let manifests = upload_hg_blobs(
                    self.ctx.clone(),
                    self.repo.clone(),
                    TreemanifestBundle2Parser::new(parts),
                    UploadBlobsType::IgnoreDuplicates,
                )
                .context("While uploading Manifest Blobs")
                .boxify()
                .compat()
                .await
                .map_err(Error::from)?;

                Ok((manifests, bundle2))
            }
            _ => Err(format_err!("Expected Bundle2 B2xTreegroup2")),
        }
    }

    /// Parse b2xinfinitepushscratchbookmarks.
    /// This part is ignored, so just parse it and forget it
    async fn maybe_resolve_infinitepush_bookmarks(
        &self,
        bundle2: OldBoxStream<Bundle2Item, Error>,
    ) -> Result<((), OldBoxStream<Bundle2Item, Error>), Error> {
        let (infinitepushbookmarks, bundle2): (
            Option<Bundle2Item>,
            OldBoxStream<Bundle2Item, Error>,
        ) = next_item(bundle2).await?;

        match infinitepushbookmarks {
            Some(Bundle2Item::B2xInfinitepushBookmarks(_, bookmarks)) => {
                let _ = bookmarks.collect().boxify().compat().await?;
                Ok(((), bundle2))
            }
            None => Ok(((), bundle2)),
            _ => Err(format_err!(
                "Expected B2xInfinitepushBookmarks or end of the stream"
            )),
        }
    }

    /// Takes parsed Changesets and scheduled for upload Filelogs and Manifests. The content of
    /// Manifests is used to figure out DAG of dependencies between a given Changeset and the
    /// Manifests and Filelogs it adds.
    /// The Changesets are scheduled for uploading and a Future is returned, whose completion means
    /// that the changesets were uploaded
    async fn upload_changesets(
        &self,
        cg_push: ChangegroupPush,
        manifests: Manifests,
    ) -> Result<UploadedBonsais, Error> {
        let changesets = toposort_changesets(cg_push.changesets)?;
        let filelogs = cg_push.filelogs;
        let content_blobs = cg_push.content_blobs;

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

        let err_context = || {
            let changesets_hashes: Vec<_> = changesets.iter().map(|(hash, _)| *hash).collect();
            ErrorKind::WhileUploadingData(changesets_hashes)
        };

        trace!(self.ctx.logger(), "changesets: {:?}", changesets);
        trace!(self.ctx.logger(), "filelogs: {:?}", filelogs.keys());
        trace!(self.ctx.logger(), "manifests: {:?}", manifests.keys());
        trace!(
            self.ctx.logger(),
            "content blobs: {:?}",
            content_blobs.keys()
        );

        // Each commit gets a future. This future polls futures of parent commits, which poll futures
        // of their parents and so on. However that might cause stackoverflow on very large pushes
        // To avoid it we commit changesets in relatively small chunks.
        let chunk_size = 100;

        let mut bonsais = UploadedBonsais::new();
        for chunk in changesets.chunks(chunk_size) {
            let mut uploaded_changesets: HashMap<HgChangesetId, ChangesetHandle> = HashMap::new();
            for (node, revlog_cs) in chunk {
                uploaded_changesets = upload_changeset(
                    self.ctx.clone(),
                    self.repo.clone(),
                    self.ctx.scuba().clone(),
                    *node,
                    revlog_cs,
                    uploaded_changesets,
                    &filelogs,
                    &manifests,
                    &content_blobs,
                    self.pushrebase_flags.casefolding_check,
                )
                .await
                .with_context(err_context)?;
            }

            let uploaded: Vec<BonsaiChangeset> = stream::iter(uploaded_changesets)
                .map(move |(_, handle)| async move {
                    let shared_item_bcs_and_something = handle
                        .get_completed_changeset()
                        .map_err(Error::from)
                        .compat()
                        .await?;

                    let bcs = shared_item_bcs_and_something.0.clone();
                    Result::<_, Error>::Ok(bcs)
                })
                .buffered(chunk_size)
                .try_collect()
                .await
                .with_context(err_context)?;

            bonsais.extend(uploaded.into_iter());
        }

        Ok(bonsais)
    }

    /// Ensures that the next item in stream is None
    async fn ensure_stream_finished(
        &self,
        bundle2: OldBoxStream<Bundle2Item, Error>,
        maybe_full_content: Option<Arc<Mutex<BytesOld>>>,
    ) -> Result<Option<RawBundle2Id>, Error> {
        let (none, _bundle2) = next_item(bundle2).await?;
        ensure!(none.is_none(), "Expected end of Bundle2");
        self.maybe_save_full_content_bundle2(maybe_full_content)
            .await
    }

    /// A method that can use any of the above maybe_resolve_* methods to return
    /// a Vec of (potentailly multiple) Part rather than an Option of Part.
    /// The original use case is to parse multiple pushkey Parts since bundle2 gets
    /// one pushkey part per bookmark.
    async fn resolve_multiple_parts<'a, T, Func, Fut>(
        &'a self,
        bundle2: OldBoxStream<Bundle2Item, Error>,
        mut maybe_resolve: Func,
    ) -> Result<(Vec<T>, OldBoxStream<Bundle2Item, Error>), Error>
    where
        Fut: Future<Output = Result<(Option<T>, OldBoxStream<Bundle2Item, Error>), Error>> + Sized,
        Func: FnMut(&'a Self, OldBoxStream<Bundle2Item, Error>) -> Fut + Send + 'static,
        T: Send + 'static,
    {
        let mut result = Vec::new();
        let mut bundle2 = bundle2;
        loop {
            let (maybe_element, rest_of_bundle2) = maybe_resolve(&self, bundle2).await?;
            bundle2 = rest_of_bundle2;
            if let Some(element) = maybe_element {
                result.push(element);
            } else {
                break;
            }
        }
        Ok((result, bundle2))
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

async fn build_changegroup_push(
    ctx: &CoreContext,
    repo: &BlobRepo,
    part_header: PartHeader,
    changesets: Changesets,
    filelogs: Filelogs,
    content_blobs: ContentBlobs,
) -> Result<ChangegroupPush, Error> {
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

            let bookmark_push = match maybe_name_res {
                Err(e) => return Err(e),
                Ok(maybe_name) => match maybe_name {
                    None => None,
                    Some(name) => {
                        let old = repo.get_bookmark(ctx.clone(), &name).compat().await?;
                        // NOTE: We do not validate that the bookmarknode selected (i.e. the
                        // changeset we should update our bookmark to) is part of the
                        // changegroup being pushed. We do however validate at a later point
                        // that this changeset exists.
                        let new = get_ascii_param(&aparams, "bookmarknode")?;
                        let new = HgChangesetId::from_ascii_str(&new)?;
                        let create = aparams.get("create").is_some();
                        let force = aparams.get("force").is_some();

                        Some(InfiniteBookmarkPush {
                            name,
                            create,
                            force,
                            old,
                            new,
                        })
                    }
                },
            };

            Some(InfinitepushPayload { bookmark_push })
        }
        _ => None,
    };

    Ok(ChangegroupPush {
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
async fn return_with_rest_of_bundle<T: Send + 'static>(
    value: T,
    unused_part: Bundle2Item,
    rest_of_bundle: OldBoxStream<Bundle2Item, Error>,
) -> Result<(T, OldBoxStream<Bundle2Item, Error>), Error> {
    Ok((
        value,
        old_stream::once(Ok(unused_part))
            .chain(rest_of_bundle)
            .boxify(),
    ))
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
        sort_topological(&cs_id_to_parents).ok_or(Error::msg("cycle in the pushed changesets!"))?;

    Ok(sorted_css
        .into_iter()
        .filter_map(|cs| changesets.remove(&cs).map(|revlog_cs| (cs, revlog_cs)))
        .collect())
}

async fn bonsai_from_hg_opt(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: Option<HgChangesetId>,
) -> Result<Option<ChangesetId>, Error> {
    match cs_id {
        None => Ok(None),
        Some(cs_id) => {
            let maybe_bcs_id = repo.get_bonsai_from_hg(ctx.clone(), cs_id).compat().await?;
            if maybe_bcs_id.is_none() {
                Err(format_err!("No bonsai mapping found for {}", cs_id))
            } else {
                Ok(maybe_bcs_id)
            }
        }
    }
}

async fn plain_hg_bookmark_push_to_bonsai(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmark_push: PlainBookmarkPush<HgChangesetId>,
) -> Result<PlainBookmarkPush<ChangesetId>, Error> {
    let PlainBookmarkPush {
        part_id,
        name,
        old,
        new,
    } = bookmark_push;

    let (old, new) = try_join!(
        bonsai_from_hg_opt(ctx, &repo, old),
        bonsai_from_hg_opt(ctx, &repo, new),
    )?;

    Ok(PlainBookmarkPush {
        part_id,
        name,
        old,
        new,
    })
}

async fn infinite_hg_bookmark_push_to_bonsai(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmark_push: InfiniteBookmarkPush<HgChangesetId>,
) -> Result<InfiniteBookmarkPush<ChangesetId>, Error> {
    let InfiniteBookmarkPush {
        name,
        force,
        create,
        old,
        new,
    } = bookmark_push;

    let (old, new) = try_join!(
        bonsai_from_hg_opt(ctx, &repo, old),
        repo.get_bonsai_from_hg(ctx.clone(), new).compat()
    )?;
    let new = match new {
        Some(new) => new,
        None => bail!("Bonsai Changeset not found"),
    };

    Ok(InfiniteBookmarkPush {
        name,
        force,
        create,
        old,
        new,
    })
}

async fn hg_pushrebase_bookmark_spec_to_bonsai(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmark_spec: PushrebaseBookmarkSpec<HgChangesetId>,
) -> Result<PushrebaseBookmarkSpec<ChangesetId>, Error> {
    let pbs = match bookmark_spec {
        PushrebaseBookmarkSpec::NormalPushrebase(onto_params) => {
            PushrebaseBookmarkSpec::NormalPushrebase(onto_params)
        }
        PushrebaseBookmarkSpec::ForcePushrebase(plain_push) => {
            PushrebaseBookmarkSpec::ForcePushrebase(
                plain_hg_bookmark_push_to_bonsai(ctx, &repo, plain_push).await?,
            )
        }
    };
    Ok(pbs)
}

async fn hg_all_bookmark_pushes_to_bonsai(
    ctx: &CoreContext,
    repo: &BlobRepo,
    all_bookmark_pushes: AllBookmarkPushes<HgChangesetId>,
) -> Result<AllBookmarkPushes<ChangesetId>, Error> {
    let abp = match all_bookmark_pushes {
        AllBookmarkPushes::PlainPushes(plain_pushes) => {
            let r =
                try_join_all(plain_pushes.into_iter().map({
                    |plain_push| plain_hg_bookmark_push_to_bonsai(ctx, &repo, plain_push)
                }))
                .await?;
            AllBookmarkPushes::PlainPushes(r)
        }
        AllBookmarkPushes::Inifinitepush(infinite_bookmark_push) => {
            let r = infinite_hg_bookmark_push_to_bonsai(ctx, &repo, infinite_bookmark_push).await?;
            AllBookmarkPushes::Inifinitepush(r)
        }
    };
    Ok(abp)
}
