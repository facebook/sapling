/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::ensure;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use ascii::AsciiString;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobrepo_hg::ChangesetHandle;
use bookmarks::BookmarkName;
use bytes::Bytes;
use context::CoreContext;
use context::SessionClass;
use core::fmt::Debug;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::future::try_join_all;
use futures::stream;
use futures::stream::BoxStream;
use futures::try_join;
use futures::Future;
use futures::StreamExt;
use futures::TryStreamExt;
use hooks::HookRejectionInfo;
use lazy_static::lazy_static;
use mercurial_bundles::Bundle2Item;
use mercurial_bundles::PartHeader;
use mercurial_bundles::PartHeaderInner;
use mercurial_bundles::PartHeaderType;
use mercurial_bundles::PartId;
use mercurial_mutation::HgMutationEntry;
use mercurial_revlog::changeset::RevlogChangeset;
use mercurial_types::HgChangesetId;
use metaconfig_types::PushrebaseFlags;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use rate_limiting::RateLimitBody;
use slog::trace;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Display;
use std::sync::Arc;
use topo_sort::sort_topological;
use tunables::tunables;
use wirepack::TreemanifestBundle2Parser;

use crate::changegroup::convert_to_revlog_changesets;
use crate::changegroup::convert_to_revlog_filelog;
use crate::changegroup::split_changegroup;
use crate::errors::*;
use crate::hook_running::make_hook_rejection_remapper;
use crate::hook_running::HookRejectionRemapper;
use crate::rate_limits::enforce_file_changes_rate_limits;
use crate::rate_limits::RateLimitedPushKind;
use crate::stats::*;
use crate::upload_blobs::upload_hg_blobs;
use crate::upload_changesets::upload_changeset;
use crate::upload_changesets::Filelogs;
use crate::upload_changesets::Manifests;

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
        total_unbundles: dynamic_timeseries("{}.total_unbundles", (reponame: String); Rate, Sum),
    }

    pub(crate) use self::STATS::*;
}

pub type Changesets = Vec<(HgChangesetId, RevlogChangeset)>;
pub type UploadedBonsais = HashSet<BonsaiChangeset>;
pub type UploadedHgChangesetIds = HashSet<HgChangesetId>;

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

pub struct HgHookRejection {
    pub(crate) hook_name: String,
    pub(crate) hg_cs_id: HgChangesetId,
    pub(crate) reason: HookRejectionInfo,
}

impl HgHookRejection {
    pub fn get_hook_name(&self) -> &str {
        &self.hook_name
    }
}

pub enum BundleResolverError {
    HookError(Vec<HgHookRejection>),
    PushrebaseConflicts(Vec<pushrebase::PushrebaseConflict>),
    Error(Error),
    RateLimitExceeded {
        limit_name: String,
        limit: RateLimitBody,
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
                            failure.hook_name, failure.hg_cs_id, failure.reason.long_description
                        )
                    })
                    .collect();
                format_err!("hooks failed:\n{}", err_msgs.join("\n"))
            }
            PushrebaseConflicts(conflicts) => {
                format_err!("pushrebase failed Conflicts({:?})", conflicts)
            }
            RateLimitExceeded {
                limit_name,
                limit,
                entity,
                value,
            } => format_err!(
                "Rate limit exceeded: {} for {}. \
                 The maximum allowed value is {} over a sliding {}s interval. \
                 If allowed, the value would be {}.",
                limit_name,
                entity,
                limit.raw_config.limit,
                limit.window.as_secs(),
                value,
            ),
            Error(err) => err,
        }
    }
}

pub trait BundleResolverResultExt<T> {
    fn context<C>(self, context: C) -> Result<T, BundleResolverError>
    where
        C: Display + Send + Sync + 'static;
}

impl<T> BundleResolverResultExt<T> for Result<T, BundleResolverError> {
    fn context<C>(self, context: C) -> Result<T, BundleResolverError>
    where
        C: Display + Send + Sync + 'static,
    {
        match self {
            Ok(v) => Ok(v),
            Err(BundleResolverError::Error(err)) => Err(err.context(context).into()),
            Err(e) => Err(e),
        }
    }
}

/// Data, needed to perform post-resolve `Push` action
pub struct PostResolvePush {
    pub changegroup_id: Option<PartId>,
    pub bookmark_pushes: Vec<PlainBookmarkPush<ChangesetId>>,
    pub mutations: Vec<HgMutationEntry>,
    pub maybe_pushvars: Option<HashMap<String, Bytes>>,
    pub non_fast_forward_policy: NonFastForwardPolicy,
    pub uploaded_bonsais: UploadedBonsais,
    pub uploaded_hg_changeset_ids: UploadedHgChangesetIds,
    pub hook_rejection_remapper: Arc<dyn HookRejectionRemapper>,
}

/// Data, needed to perform post-resolve `InfinitePush` action
pub struct PostResolveInfinitePush {
    pub changegroup_id: Option<PartId>,
    pub maybe_bookmark_push: Option<InfiniteBookmarkPush<ChangesetId>>,
    pub mutations: Vec<HgMutationEntry>,
    pub uploaded_bonsais: UploadedBonsais,
    pub uploaded_hg_changeset_ids: UploadedHgChangesetIds,
}

/// Data, needed to perform post-resolve `PushRebase` action
#[derive(Clone)]
pub struct PostResolvePushRebase {
    pub bookmark_push_part_id: Option<PartId>,
    pub bookmark_spec: PushrebaseBookmarkSpec<ChangesetId>,
    pub maybe_pushvars: Option<HashMap<String, Bytes>>,
    pub commonheads: CommonHeads,
    pub uploaded_bonsais: UploadedBonsais,
    pub hook_rejection_remapper: Arc<dyn HookRejectionRemapper>,
}

/// Data, needed to perform post-resolve `BookmarkOnlyPushRebase` action
pub struct PostResolveBookmarkOnlyPushRebase {
    pub bookmark_push: PlainBookmarkPush<ChangesetId>,
    pub maybe_pushvars: Option<HashMap<String, Bytes>>,
    pub non_fast_forward_policy: NonFastForwardPolicy,
    pub hook_rejection_remapper: Arc<dyn HookRejectionRemapper>,
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
    bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
    pure_push_allowed: bool,
    pushrebase_flags: PushrebaseFlags,
    maybe_backup_repo_source: Option<BlobRepo>,
) -> Result<PostResolveAction, BundleResolverError> {
    let result = resolve_impl(
        ctx,
        repo,
        infinitepush_writes_allowed,
        bundle2,
        pure_push_allowed,
        pushrebase_flags,
        maybe_backup_repo_source,
    )
    .await;
    report_unbundle_type(ctx, repo, &result);
    result
}

async fn resolve_impl<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    infinitepush_writes_allowed: bool,
    bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
    pure_push_allowed: bool,
    pushrebase_flags: PushrebaseFlags,
    maybe_backup_repo_source: Option<BlobRepo>,
) -> Result<PostResolveAction, BundleResolverError> {
    let resolver = Bundle2Resolver::new(ctx, repo, infinitepush_writes_allowed, pushrebase_flags);
    let bundle2 = resolver.resolve_stream_params(bundle2).await?;
    let bundle2 = resolver.resolve_replycaps(bundle2).await?;

    let (maybe_commonheads, bundle2) = resolver.maybe_resolve_commonheads(bundle2).await?;
    let (maybe_pushvars, bundle2) = resolver
        .maybe_resolve_pushvars(bundle2)
        .await
        .context("While resolving Pushvars")?;

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
                maybe_pushvars,
                non_fast_forward_policy,
            )
            .await
            .map_err(BundleResolverError::from)
        } else {
            fn changegroup_always_unacceptable() -> bool {
                false
            }
            resolve_pushrebase(
                ctx,
                commonheads,
                resolver,
                bundle2,
                maybe_pushvars,
                changegroup_always_unacceptable,
                maybe_backup_repo_source,
            )
            .await
        }
    } else {
        resolve_push(
            ctx,
            resolver,
            bundle2,
            maybe_pushvars,
            non_fast_forward_policy,
            move || pure_push_allowed,
            maybe_backup_repo_source,
        )
        .await
        .context("bundle2_resolver error")
        .map_err(BundleResolverError::from)
    };

    match post_resolve_action {
        Err(e) => Err(e),
        Ok(val) => Ok(val),
    }
}

fn report_unbundle_type(
    ctx: &CoreContext,
    repo: &BlobRepo,
    post_resolve_action: &Result<PostResolveAction, BundleResolverError>,
) {
    let repo_name = repo.name().clone();
    UNBUNDLE_STATS::total_unbundles.add_value(1, (repo_name.clone(),));
    let unbundle_resolved = "Unbundle resolved";
    match post_resolve_action {
        Ok(PostResolveAction::Push(_)) => {
            ctx.scuba()
                .clone()
                .log_with_msg(unbundle_resolved, Some("push".to_owned()));
            UNBUNDLE_STATS::push.add_value(1, (repo_name,))
        }
        Ok(PostResolveAction::PushRebase(_)) => {
            ctx.scuba()
                .clone()
                .log_with_msg(unbundle_resolved, Some("pushrebase".to_owned()));
            UNBUNDLE_STATS::pushrebase.add_value(1, (repo_name,))
        }
        Ok(PostResolveAction::InfinitePush(_)) => {
            ctx.scuba()
                .clone()
                .log_with_msg(unbundle_resolved, Some("infinitepush".to_owned()));
            UNBUNDLE_STATS::infinitepush.add_value(1, (repo_name,))
        }
        Ok(PostResolveAction::BookmarkOnlyPushRebase(_)) => {
            ctx.scuba().clone().log_with_msg(
                unbundle_resolved,
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
    bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
    maybe_pushvars: Option<HashMap<String, Bytes>>,
    non_fast_forward_policy: NonFastForwardPolicy,
    changegroup_acceptable: impl FnOnce() -> bool + Send + Sync + 'static,
    maybe_backup_repo_source: Option<BlobRepo>,
) -> Result<PostResolveAction, Error> {
    let (cg_push, bundle2) = resolver
        .maybe_resolve_changegroup(bundle2, changegroup_acceptable)
        .await
        .context("While resolving Changegroup")?;
    let (pushkeys, bundle2) = resolver
        .resolve_multiple_parts(bundle2, Bundle2Resolver::maybe_resolve_pushkey)
        .await
        .context("While resolving Pushkey")?;
    let is_infinitepush = cg_push
        .as_ref()
        .and_then(|cg_push| cg_push.infinitepush_payload.as_ref())
        .is_some();
    let infinitepush_bp = cg_push
        .as_ref()
        .and_then(|cg_push| cg_push.infinitepush_payload.as_ref())
        .and_then(|ip_payload| ip_payload.bookmark_push.as_ref());

    let infinitepush_bp = infinitepush_bp.cloned();

    let (mutations, bundle2) = resolver
        .maybe_resolve_infinitepush_mutation(bundle2)
        .await
        .context("While resolving InfinitepushMutation")?;

    let (cg_and_manifests, bundle2) = if let Some(cg_push) = cg_push {
        let (manifests, bundle2) = resolver
            .resolve_b2xtreegroup2(bundle2)
            .await
            .context("While resolving B2xTreegroup2")?;
        (Some((cg_push, manifests)), bundle2)
    } else {
        (None, bundle2)
    };
    // At the moment pushkey part may appear in two places: after Changegroup
    // and after Treegroup.
    let (pushkeys, bundle2) = if pushkeys.is_empty() {
        resolver
            .resolve_multiple_parts(bundle2, Bundle2Resolver::maybe_resolve_pushkey)
            .await
            .context("While resolving Pushkey")?
    } else {
        (pushkeys, bundle2)
    };
    let maybe_hg_bookmark_push = try_collect_all_bookmark_pushes(pushkeys, infinitepush_bp)?;

    let (changegroup_id, uploaded_bonsais, uploaded_hg_changeset_ids) =
        if let Some((cg_push, manifests)) = cg_and_manifests {
            let changegroup_id = Some(cg_push.part_id);
            let (uploaded_bonsais, uploaded_hg_changeset_ids) = resolver
                .upload_changesets(cg_push, manifests, maybe_backup_repo_source)
                .await?;
            // Note: we do not run hooks on pure pushes. This probably has to be changed later.
            (changegroup_id, uploaded_bonsais, uploaded_hg_changeset_ids)
        } else {
            (None, UploadedBonsais::new(), UploadedHgChangesetIds::new())
        };

    let ((), bundle2) = resolver
        .maybe_resolve_infinitepush_bookmarks(bundle2)
        .await
        .context("While resolving B2xInfinitepushBookmarks")?;
    resolver.ensure_stream_finished(bundle2).await?;

    let maybe_bonsai_bookmark_push = match maybe_hg_bookmark_push {
        Some(hg_bookmark_push) => {
            Some(hg_all_bookmark_pushes_to_bonsai(ctx, resolver.repo, hg_bookmark_push).await?)
        }
        None => None,
    };

    if is_infinitepush {
        get_post_resolve_infinitepush(
            changegroup_id,
            maybe_bonsai_bookmark_push,
            mutations,
            uploaded_bonsais,
            uploaded_hg_changeset_ids,
        )
        .map(PostResolveAction::InfinitePush)
    } else {
        let hook_rejection_remapper =
            make_hook_rejection_remapper(ctx, resolver.repo.clone()).into();

        get_post_resolve_push(
            changegroup_id,
            maybe_bonsai_bookmark_push,
            mutations,
            maybe_pushvars,
            non_fast_forward_policy,
            uploaded_bonsais,
            uploaded_hg_changeset_ids,
            hook_rejection_remapper,
        )
        .map(PostResolveAction::Push)
    }
}

fn get_post_resolve_infinitepush(
    changegroup_id: Option<PartId>,
    maybe_bonsai_bookmark_push: Option<AllBookmarkPushes<ChangesetId>>,
    mutations: Vec<HgMutationEntry>,
    uploaded_bonsais: UploadedBonsais,
    uploaded_hg_changeset_ids: UploadedHgChangesetIds,
) -> Result<PostResolveInfinitePush, Error> {
    let maybe_bookmark_push = match maybe_bonsai_bookmark_push {
        Some(AllBookmarkPushes::PlainPushes(_)) => {
            return Err(format_err!(
                "Infinitepush push cannot contain regular bookmarks"
            ));
        }
        Some(AllBookmarkPushes::Inifinitepush(bookmark_push)) => Some(bookmark_push),
        None => None,
    };

    Ok(PostResolveInfinitePush {
        changegroup_id,
        maybe_bookmark_push,
        mutations,
        uploaded_bonsais,
        uploaded_hg_changeset_ids,
    })
}

fn get_post_resolve_push(
    changegroup_id: Option<PartId>,
    maybe_bonsai_bookmark_push: Option<AllBookmarkPushes<ChangesetId>>,
    mutations: Vec<HgMutationEntry>,
    maybe_pushvars: Option<HashMap<String, Bytes>>,
    non_fast_forward_policy: NonFastForwardPolicy,
    uploaded_bonsais: UploadedBonsais,
    uploaded_hg_changeset_ids: UploadedHgChangesetIds,
    hook_rejection_remapper: Arc<dyn HookRejectionRemapper>,
) -> Result<PostResolvePush, Error> {
    let bookmark_pushes = match maybe_bonsai_bookmark_push {
        Some(AllBookmarkPushes::Inifinitepush(_bookmark_push)) => {
            return Err(format_err!(
                "This should actually be impossible: non-infinitepush push with infinitepush bookmarks"
            ));
        }
        Some(AllBookmarkPushes::PlainPushes(bookmark_pushes)) => bookmark_pushes,
        None => vec![],
    };

    Ok(PostResolvePush {
        changegroup_id,
        bookmark_pushes,
        mutations,
        maybe_pushvars,
        non_fast_forward_policy,
        uploaded_bonsais,
        uploaded_hg_changeset_ids,
        hook_rejection_remapper,
    })
}

// Enum used to pass data for normal or forceful pushrebases
// Normal pushrebase is what one would expect: take a (potentially
// stack of) commit(s) and rebase it on top of a given bookmark.
// Force pushrebase is basically a push, which for logging
// and respondin purposes is treated like a pushrebase
#[derive(Clone)]
pub enum PushrebaseBookmarkSpec<T: Copy> {
    NormalPushrebase(BookmarkName),
    ForcePushrebase(PlainBookmarkPush<T>),
}

impl<T: Copy> PushrebaseBookmarkSpec<T> {
    pub fn get_bookmark_name(&self) -> &BookmarkName {
        match self {
            PushrebaseBookmarkSpec::NormalPushrebase(onto_bookmark) => onto_bookmark,
            PushrebaseBookmarkSpec::ForcePushrebase(plain_push) => &plain_push.name,
        }
    }
}

async fn resolve_pushrebase<'r>(
    ctx: &'r CoreContext,
    commonheads: CommonHeads,
    resolver: Bundle2Resolver<'r>,
    bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
    maybe_pushvars: Option<HashMap<String, Bytes>>,
    changegroup_acceptable: impl FnOnce() -> bool + Send + Sync + 'static,
    maybe_backup_repo_source: Option<BlobRepo>,
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
    let onto_bookmark = match cg_push.mparams.get("onto") {
        Some(onto_bookmark) => {
            let v = Vec::from(onto_bookmark.as_ref());
            let onto_bookmark = String::from_utf8(v).map_err(Error::from)?;

            BookmarkName::new(onto_bookmark)?
        }
        None => return Err(format_err!("onto is not specified").into()),
    };

    let will_rebase = onto_bookmark != *DONOTREBASEBOOKMARK;
    // Mutation information must not be present in public commits
    // See T54101162, S186586
    if !will_rebase {
        for (_, hg_cs) in &cg_push.changesets {
            for key in pushrebase::MUTATION_KEYS {
                if hg_cs.extra.as_ref().contains_key(key.as_bytes()) {
                    return Err(Error::msg("Forced push blocked because it contains mutation metadata.\n\
                                You can remove the metadata from a commit with `hg amend --config mutation.record=false`.\n\
                                For more help, please contact the Source Control team at https://fburl.com/27qnuyl2").into());
                }
            }
        }
    }

    let (uploaded_bonsais, _uploaded_hg_changeset_ids) = resolver
        .upload_changesets(cg_push, manifests, maybe_backup_repo_source)
        .await?;

    let (pushkeys, bundle2) = resolver
        .resolve_multiple_parts(bundle2, Bundle2Resolver::maybe_resolve_pushkey)
        .await
        .context("While resolving Pushkey")?;

    let bookmark_pushes = collect_pushkey_bookmark_pushes(pushkeys);
    if bookmark_pushes.len() > 1 {
        return Err(format_err!("Too many pushkey parts: {:?}", bookmark_pushes).into());
    }

    let (bookmark_push_part_id, bookmark_spec) = match bookmark_pushes.into_iter().next() {
        Some(bk_push) if bk_push.name != onto_bookmark && onto_bookmark != *DONOTREBASEBOOKMARK => {
            return Err(format_err!(
                "allowed only pushes of {} bookmark: {:?}",
                onto_bookmark,
                bk_push
            )
            .into());
        }
        Some(bk_push) if onto_bookmark == *DONOTREBASEBOOKMARK => {
            (
                // This is a force pushrebase scenario. We need to ignore `onto_bookmark`
                // and run normal push (using bk_push), but generate a pushrebase
                // response.
                // See comment next to DONOTREBASEBOOKMARK definition
                Some(bk_push.part_id),
                PushrebaseBookmarkSpec::ForcePushrebase(bk_push),
            )
        }
        Some(bk_push) => (
            Some(bk_push.part_id),
            PushrebaseBookmarkSpec::NormalPushrebase(onto_bookmark),
        ),
        None => (
            None,
            PushrebaseBookmarkSpec::NormalPushrebase(onto_bookmark),
        ),
    };

    resolver.ensure_stream_finished(bundle2).await?;
    let bookmark_spec =
        hg_pushrebase_bookmark_spec_to_bonsai(ctx, resolver.repo, bookmark_spec).await?;

    let hook_rejection_remapper = make_hook_rejection_remapper(ctx, resolver.repo.clone()).into();

    Ok(PostResolveAction::PushRebase(PostResolvePushRebase {
        bookmark_push_part_id,
        bookmark_spec,
        maybe_pushvars,
        commonheads,
        uploaded_bonsais,
        hook_rejection_remapper,
    }))
}

/// Do the right thing when pushrebase-enabled client only wants to manipulate bookmarks
async fn resolve_bookmark_only_pushrebase<'r>(
    ctx: &'r CoreContext,
    resolver: Bundle2Resolver<'r>,
    bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
    maybe_pushvars: Option<HashMap<String, Bytes>>,
    non_fast_forward_policy: NonFastForwardPolicy,
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
    resolver.ensure_stream_finished(bundle2).await?;
    let bookmark_push = plain_hg_bookmark_push_to_bonsai(ctx, resolver.repo, bookmark_push).await?;
    let hook_rejection_remapper = make_hook_rejection_remapper(ctx, resolver.repo.clone()).into();

    Ok(PostResolveAction::BookmarkOnlyPushRebase(
        PostResolveBookmarkOnlyPushRebase {
            bookmark_push,
            maybe_pushvars,
            non_fast_forward_policy,
            hook_rejection_remapper,
        },
    ))
}

/// Represents all the bookmark pushes that are created
/// by a single unbundle wireproto command. This can
/// be either an exactly one infinitepush, or multiple
/// plain pushes
enum AllBookmarkPushes<T: Copy> {
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
    mparams: HashMap<String, Bytes>,
    /// Infinitepush data provided through the Changegroup. If the push was an Infinitepush, this
    /// will be present.
    infinitepush_payload: Option<InfinitepushPayload>,
}

#[derive(Clone)]
pub struct CommonHeads {
    pub heads: Vec<HgChangesetId>,
}

enum Pushkey {
    HgBookmarkPush(PlainBookmarkPush<HgChangesetId>),
    Phases,
}

/// Holds repo and logger for convienience access from it's methods
#[derive(Clone)]
struct Bundle2Resolver<'r> {
    ctx: &'r CoreContext,
    repo: &'r BlobRepo,
    infinitepush_writes_allowed: bool,
    #[allow(dead_code)]
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
        mut bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
    ) -> Result<(bool, BoxStream<'static, Result<Bundle2Item<'static>>>)> {
        match bundle2.try_next().await? {
            Some(part) => {
                if let Bundle2Item::Pushkey(header, box_future) = part {
                    Ok((
                        true,
                        stream::once(async { Ok(Bundle2Item::Pushkey(header, box_future)) })
                            .chain(bundle2)
                            .boxed(),
                    ))
                } else {
                    return_with_rest_of_bundle(false, part, bundle2).await
                }
            }
            _ => Ok((false, bundle2)),
        }
    }

    /// Parse the stream header and extract stream params from it
    /// Return the rest of the stream along, excluding the params
    async fn resolve_stream_params(
        &self,
        mut bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
    ) -> Result<BoxStream<'static, Result<Bundle2Item<'static>>>> {
        match bundle2.try_next().await? {
            Some(Bundle2Item::Start(_)) => Ok(bundle2),
            _ => Err(format_err!("Expected Bundle2 Start")),
        }
    }

    /// Parse replycaps and ignore its content
    /// Return the rest of the bundle
    async fn resolve_replycaps(
        &self,
        mut bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
    ) -> Result<BoxStream<'static, Result<Bundle2Item<'static>>>> {
        match bundle2.try_next().await? {
            Some(Bundle2Item::Replycaps(_, part)) => {
                part.await?;
                Ok(bundle2)
            }
            _ => Err(format_err!("Expected Bundle2 Replycaps")),
        }
    }

    // Parse b2x:commonheads
    // This part sent by pushrebase so that server can find out what commits to send back to the
    // client. This part is used as a marker that this push is pushrebase.
    async fn maybe_resolve_commonheads(
        &self,
        mut bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
    ) -> Result<
        (
            Option<CommonHeads>,
            BoxStream<'static, Result<Bundle2Item<'static>>>,
        ),
        Error,
    > {
        match bundle2.try_next().await? {
            Some(Bundle2Item::B2xCommonHeads(_header, heads)) => Ok((
                Some(CommonHeads {
                    heads: heads.try_collect().await?,
                }),
                bundle2,
            )),

            Some(part) => return_with_rest_of_bundle(None, part, bundle2).await,
            _ => Err(format_err!("Unexpected Bundle2 stream end")),
        }
    }

    /// Parse pushvars
    /// It is used to store hook arguments.
    async fn maybe_resolve_pushvars(
        &self,
        mut bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
    ) -> Result<
        (
            Option<HashMap<String, Bytes>>,
            BoxStream<'static, Result<Bundle2Item<'static>>>,
        ),
        Error,
    > {
        let maybe_pushvars = match bundle2.try_next().await? {
            Some(Bundle2Item::Pushvars(header, emptypart)) => {
                let pushvars = header.into_inner().aparams;
                // ignored for now, will be used for hooks
                emptypart.await?;
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
        mut bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
        changegroup_acceptable: impl FnOnce() -> bool + Send + Sync + 'static,
    ) -> Result<
        (
            Option<ChangegroupPush>,
            BoxStream<'static, Result<Bundle2Item<'static>>>,
        ),
        Error,
    > {
        let infinitepush_writes_allowed = self.infinitepush_writes_allowed;

        let maybe_cg_push: Option<ChangegroupPush> = match bundle2.try_next().await? {
            // XXX: we may be interested in checking that this is a correct changegroup part
            // type
            Some(Bundle2Item::Changegroup(header, parts))
            | Some(Bundle2Item::B2xInfinitepush(header, parts))
            | Some(Bundle2Item::B2xRebase(header, parts)) => {
                if header.part_type() == &PartHeaderType::Changegroup && !changegroup_acceptable() {
                    // Changegroup part type signals that we are in a pure push scenario
                    return Err(format_err!("Pure pushes are disallowed in this repo"));
                }

                let is_infinitepush = header.part_type() == &PartHeaderType::B2xInfinitepush;

                if is_infinitepush && !infinitepush_writes_allowed {
                    bail!(
                        "Infinitepush is not enabled on this server. Contact Source Control @ FB."
                    );
                }

                let push_kind = if is_infinitepush {
                    RateLimitedPushKind::InfinitePush
                } else {
                    RateLimitedPushKind::Public
                };

                let (changesets, filelogs) = split_changegroup(parts);
                let changesets: Vec<(HgChangesetId, RevlogChangeset)> =
                    convert_to_revlog_changesets(changesets)
                        .try_collect()
                        .await?;

                let commit_limit = tunables().get_unbundle_limit_num_of_commits_in_push();
                // Ignore commit limit if hg sync job is pushing. Hg sync job is used
                // to mirror one repository into another, and we can't discard a push
                // even if it's too big
                if commit_limit > 0 && !self.ctx.session().is_hg_sync_job() {
                    let commit_limit: usize = commit_limit.try_into().unwrap();
                    if changesets.len() > commit_limit {
                        bail!(
                            "Trying to push too many commits! Limit is {}, tried to push {}",
                            commit_limit,
                            changesets.len()
                        );
                    }
                }

                let changesets = if is_infinitepush
                    && tunables().get_filter_pre_existing_commits_on_infinitepush()
                {
                    let hg_cs_ids = changesets.iter().map(|(id, _)| *id).collect::<Vec<_>>();

                    let mapping = self
                        .repo
                        .get_hg_bonsai_mapping(self.ctx.clone(), hg_cs_ids)
                        .await
                        .with_context(|| "Failed to query for pre-existing changesets")?;

                    let existing = mapping
                        .into_iter()
                        .map(|(hg_cs_id, _)| hg_cs_id)
                        .collect::<HashSet<_>>();

                    let orig_count = changesets.len();

                    let new_changesets = changesets
                        .into_iter()
                        .filter(|(hg_cs_id, _)| !existing.contains(hg_cs_id))
                        .collect::<Vec<_>>();

                    if new_changesets.len() < orig_count {
                        self.ctx
                            .scuba()
                            .clone()
                            .add("original_changeset_count", orig_count)
                            .add("new_changeset_count", new_changesets.len())
                            .log_with_msg("Filtered out pre-existing changesets", None);
                    }

                    new_changesets
                } else {
                    changesets
                };

                enforce_file_changes_rate_limits(
                    self.ctx,
                    push_kind,
                    changesets.iter().map(|(_, rc)| rc),
                )
                .await?;

                let mut ctx = self.ctx.clone();
                if is_infinitepush
                    && tunables::tunables().get_commit_cloud_use_background_session_class()
                {
                    ctx.session_mut()
                        .override_session_class(SessionClass::BackgroundUnlessTooSlow);
                }

                let filelogs = upload_hg_blobs(
                    &ctx,
                    self.repo,
                    convert_to_revlog_filelog(self.ctx.clone(), self.repo.clone(), filelogs),
                )
                .await
                .context("While uploading File Blobs")?;

                let cg_push =
                    build_changegroup_push(self.ctx, self.repo, header, changesets, filelogs)
                        .await?;

                Some(cg_push)
            }
            Some(part) => return return_with_rest_of_bundle(None, part, bundle2).await,
            _ => return Err(format_err!("Unexpected Bundle2 stream end")),
        };

        Ok((maybe_cg_push, bundle2))
    }

    /// Parses pushkey part if it exists
    /// Returns an error if the pushkey namespace is unknown
    async fn maybe_resolve_pushkey(
        &self,
        mut bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
    ) -> Result<(
        Option<Pushkey>,
        BoxStream<'static, Result<Bundle2Item<'static>>>,
    )> {
        match bundle2.try_next().await? {
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

                emptypart.await?;
                Ok((Some(pushkey), bundle2))
            }
            Some(part) => return_with_rest_of_bundle(None, part, bundle2).await,
            None => Ok((None, bundle2)),
        }
    }

    /// Parse b2xinfinitepushmutation.
    async fn maybe_resolve_infinitepush_mutation(
        &self,
        mut bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
    ) -> Result<
        (
            Vec<HgMutationEntry>,
            BoxStream<'static, Result<Bundle2Item<'static>>>,
        ),
        Error,
    > {
        match bundle2.try_next().await? {
            Some(Bundle2Item::B2xInfinitepushMutation(_, entries)) => {
                let mutations = entries.try_concat().await?;
                Ok((mutations, bundle2))
            }
            Some(part) => return_with_rest_of_bundle(Vec::new(), part, bundle2).await,
            None => Ok((Vec::new(), bundle2)),
        }
    }

    /// Parse b2xtreegroup2.
    /// The Manifests should be scheduled for uploading to BlobRepo and the Future resolving in
    /// their upload as well as their parsed content should be used for uploading changesets.
    async fn resolve_b2xtreegroup2(
        &self,
        mut bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
    ) -> Result<(Manifests, BoxStream<'static, Result<Bundle2Item<'static>>>)> {
        match bundle2.try_next().await? {
            Some(Bundle2Item::B2xTreegroup2(_, parts))
            | Some(Bundle2Item::B2xRebasePack(_, parts)) => {
                let manifests = upload_hg_blobs(
                    self.ctx,
                    self.repo,
                    TreemanifestBundle2Parser::new(parts).compat(),
                )
                .await
                .context("While uploading Manifest Blobs")?;

                Ok((manifests, bundle2))
            }
            _ => Err(format_err!("Expected Bundle2 B2xTreegroup2")),
        }
    }

    /// Parse b2xinfinitepushscratchbookmarks.
    /// This part is ignored, so just parse it and forget it
    async fn maybe_resolve_infinitepush_bookmarks(
        &self,
        mut bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
    ) -> Result<((), BoxStream<'static, Result<Bundle2Item<'static>>>)> {
        match bundle2.try_next().await? {
            Some(Bundle2Item::B2xInfinitepushBookmarks(_, bookmarks)) => {
                bookmarks.try_for_each(|_| future::ok(())).await?;
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
        maybe_backup_repo_source: Option<BlobRepo>,
    ) -> Result<(UploadedBonsais, UploadedHgChangesetIds), Error> {
        let changesets = toposort_changesets(cg_push.changesets)?;
        let filelogs = cg_push.filelogs;

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

        let err_context = || {
            let changesets_hashes: Vec<_> = changesets.iter().map(|(hash, _)| *hash).collect();
            ErrorKind::WhileUploadingData(changesets_hashes)
        };

        trace!(self.ctx.logger(), "changesets: {:?}", changesets);
        trace!(self.ctx.logger(), "filelogs: {:?}", filelogs.keys());
        trace!(self.ctx.logger(), "manifests: {:?}", manifests.keys());

        // Each commit gets a future. This future polls futures of parent commits, which poll futures
        // of their parents and so on. However that might cause stackoverflow on very large pushes
        // To avoid it we commit changesets in relatively small chunks.
        let chunk_size = 100;

        let mut bonsais = UploadedBonsais::new();
        let mut hg_cs_ids = UploadedHgChangesetIds::new();
        for chunk in changesets.chunks(chunk_size) {
            let mut uploaded_changesets: HashMap<HgChangesetId, ChangesetHandle> = HashMap::new();
            for (node, revlog_cs) in chunk {
                uploaded_changesets = upload_changeset(
                    self.ctx.clone(),
                    None, // No logging to scribe happens through this codepath
                    self.repo.clone(),
                    self.ctx.scuba().clone(),
                    *node,
                    revlog_cs,
                    uploaded_changesets,
                    &filelogs,
                    &manifests,
                    maybe_backup_repo_source.clone(),
                )
                .await
                .with_context(err_context)?;
            }

            let uploaded: Vec<(BonsaiChangeset, HgChangesetId)> = stream::iter(uploaded_changesets)
                .map(move |(hg_cs_id, handle): (HgChangesetId, _)| async move {
                    let shared_item_bcs_and_something = handle.get_completed_changeset().await?;

                    let bcs = shared_item_bcs_and_something.0;
                    Result::<_, Error>::Ok((bcs, hg_cs_id))
                })
                .buffered(chunk_size)
                .try_collect()
                .await
                .with_context(err_context)?;

            bonsais.reserve(uploaded.len());
            hg_cs_ids.reserve(uploaded.len());
            for (bcs, hg_cs_id) in uploaded {
                bonsais.insert(bcs);
                hg_cs_ids.insert(hg_cs_id);
            }
        }

        Ok((bonsais, hg_cs_ids))
    }

    /// Ensures that the next item in stream is None
    async fn ensure_stream_finished(
        &self,
        mut bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
    ) -> Result<(), Error> {
        ensure!(
            bundle2.try_next().await?.is_none(),
            "Expected end of Bundle2"
        );
        Ok(())
    }

    /// A method that can use any of the above maybe_resolve_* methods to return
    /// a Vec of (potentailly multiple) Part rather than an Option of Part.
    /// The original use case is to parse multiple pushkey Parts since bundle2 gets
    /// one pushkey part per bookmark.
    async fn resolve_multiple_parts<'a, T, Func, Fut>(
        &'a self,
        bundle2: BoxStream<'static, Result<Bundle2Item<'static>>>,
        mut maybe_resolve: Func,
    ) -> Result<(Vec<T>, BoxStream<'static, Result<Bundle2Item<'static>>>)>
    where
        Fut: Future<Output = Result<(Option<T>, BoxStream<'static, Result<Bundle2Item<'static>>>)>>
            + Sized,
        Func: FnMut(&'a Self, BoxStream<'static, Result<Bundle2Item<'static>>>) -> Fut
            + Send
            + 'static,
        T: Send + 'static,
    {
        let mut result = Vec::new();
        let mut bundle2 = bundle2;
        loop {
            let (maybe_element, rest_of_bundle2) = maybe_resolve(self, bundle2).await?;
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
                        let old = repo.get_bookmark(ctx.clone(), &name).await?;
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
) -> Result<Option<AllBookmarkPushes<HgChangesetId>>> {
    let bookmark_pushes: Vec<_> = collect_pushkey_bookmark_pushes(pushkeys)
        .into_iter()
        .collect();
    let bookmark_pushes_len = bookmark_pushes.len();
    match (bookmark_pushes_len, infinitepush_bookmark_push) {
        (0, Some(infinitepush_bookmark_push)) => {
            STATS::bookmark_pushkeys_count.add_value(1);
            Ok(Some(AllBookmarkPushes::Inifinitepush(
                infinitepush_bookmark_push,
            )))
        }
        (bookmark_pushes_len, None) if bookmark_pushes_len > 0 => {
            STATS::bookmark_pushkeys_count.add_value(bookmark_pushes_len as i64);
            Ok(Some(AllBookmarkPushes::PlainPushes(bookmark_pushes)))
        }
        // Neither plain, not infinitepush bookmark pushes are present
        (0, None) => Ok(None),
        (_, Some(_)) => Err(format_err!(
            "Same bundle2 can not be used for both plain and infinite push"
        )),
        (_, _) => Err(format_err!("An unreachable pattern. Programmer's error")),
    }
}

/// Helper fn to return some (usually "empty") value and
/// chain together an unused part with the rest of the bundle
async fn return_with_rest_of_bundle<T: Send + 'static>(
    value: T,
    unused_part: Bundle2Item<'static>,
    rest_of_bundle: BoxStream<'static, Result<Bundle2Item<'static>>>,
) -> Result<(T, BoxStream<'static, Result<Bundle2Item<'static>>>)> {
    Ok((
        value,
        stream::once(async { Ok(unused_part) })
            .chain(rest_of_bundle)
            .boxed(),
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
            let maybe_bcs_id = repo
                .bonsai_hg_mapping()
                .get_bonsai_from_hg(ctx, cs_id)
                .await?;
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
        bonsai_from_hg_opt(ctx, repo, old),
        bonsai_from_hg_opt(ctx, repo, new),
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
        bonsai_from_hg_opt(ctx, repo, old),
        repo.bonsai_hg_mapping().get_bonsai_from_hg(ctx, new)
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
        PushrebaseBookmarkSpec::NormalPushrebase(onto_bookmark) => {
            PushrebaseBookmarkSpec::NormalPushrebase(onto_bookmark)
        }
        PushrebaseBookmarkSpec::ForcePushrebase(plain_push) => {
            PushrebaseBookmarkSpec::ForcePushrebase(
                plain_hg_bookmark_push_to_bonsai(ctx, repo, plain_push).await?,
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
            let r = try_join_all(
                plain_pushes
                    .into_iter()
                    .map({ |plain_push| plain_hg_bookmark_push_to_bonsai(ctx, repo, plain_push) }),
            )
            .await?;
            AllBookmarkPushes::PlainPushes(r)
        }
        AllBookmarkPushes::Inifinitepush(infinite_bookmark_push) => {
            let r = infinite_hg_bookmark_push_to_bonsai(ctx, repo, infinite_bookmark_push).await?;
            AllBookmarkPushes::Inifinitepush(r)
        }
    };
    Ok(abp)
}
