/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{BundleResolverError, PostResolveAction, PostResolvePush, PostResolvePushRebase};
use anyhow::{anyhow, Result};
use context::CoreContext;
use futures::{
    future::{try_join_all, BoxFuture},
    FutureExt,
};
use limits::types::{RateLimit, RateLimitStatus};
use maplit::hashmap;
use mercurial_revlog::changeset::RevlogChangeset;
use mononoke_types::{BonsaiChangeset, RepositoryId};
use scuba_ext::ScubaSampleBuilderExt;
use sha2::{Digest, Sha256};
use slog::debug;
use std::collections::HashMap;
use std::time::Duration;
use time_window_counter::{BoxGlobalTimeWindowCounter, GlobalTimeWindowCounterBuilder};
use tokio::time::timeout;

const TIME_WINDOW_MIN: u32 = 10;
const TIME_WINDOW_MAX: u32 = 3600;

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub(crate) enum RateLimitedPushKind {
    Public,
    InfinitePush,
}

impl std::fmt::Display for RateLimitedPushKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Public => write!(f, "Public"),
            Self::InfinitePush => write!(f, "Infinitepush"),
        }
    }
}

fn get_file_changes_rate_limit(
    ctx: &CoreContext,
    repo_id: RepositoryId,
    push_kind: RateLimitedPushKind,
) -> Option<(RateLimit, &str)> {
    let maybe_rate_limit_with_category = ctx
        .session()
        .load_limiter()
        .and_then(|load_limiter| {
            let maybe_limit_map = if push_kind == RateLimitedPushKind::InfinitePush {
                load_limiter
                    .rate_limits()
                    .infinitepush_file_changes
                    .as_ref()
            } else {
                load_limiter.rate_limits().public_file_changes.as_ref()
            };

            let category = load_limiter.category();

            maybe_limit_map.map(|limit_map| (limit_map, category))
        })
        .and_then(|(limit_map, category)| {
            limit_map
                .get(&(repo_id.id() as i64))
                .map(|limit| (limit.clone(), category))
        });

    if maybe_rate_limit_with_category.is_none() {
        debug!(
            ctx.logger(),
            "{} is not rate-limited for {:?}", repo_id, push_kind
        );
    }

    maybe_rate_limit_with_category
}

pub(crate) async fn enforce_file_changes_rate_limits<
    'a,
    RC: Iterator<Item = &'a RevlogChangeset>,
>(
    ctx: &CoreContext,
    repo_id: RepositoryId,
    push_kind: RateLimitedPushKind,
    revlog_changesets: RC,
) -> Result<(), BundleResolverError> {
    let (limit, category) = match get_file_changes_rate_limit(ctx, repo_id, push_kind) {
        Some((limit, category)) => (limit, category),
        None => return Ok(()),
    };

    let enforced = match limit.status {
        RateLimitStatus::Disabled => return Ok(()),
        RateLimitStatus::Tracked => false,
        RateLimitStatus::Enforced => true,
        // NOTE: Thrift enums aren't real enums once in Rust. We have to account for other values
        // here.
        _ => {
            let e = anyhow!("Invalid file count rate limit status: {:?}", limit.status);
            return Err(BundleResolverError::Error(e));
        }
    };


    let max_value = limit.max_value as f64;
    let interval = limit.interval as u32;
    let key = format!("{}_{}", limit.prefix, repo_id);
    let timeout_dur = Duration::from_secs(limit.timeout as u64);

    let counter = GlobalTimeWindowCounterBuilder::build(
        ctx.fb,
        category,
        key,
        TIME_WINDOW_MIN,
        TIME_WINDOW_MAX,
    );
    let total_file_number: usize = revlog_changesets.map(|rc| rc.files().len()).sum();
    {
        let push_kind = format!("{}", push_kind);
        counter_check_and_bump(
            ctx,
            counter,
            max_value,
            interval,
            timeout_dur,
            total_file_number as f64,
            enforced,
            "File Changes Rate Limit",        /* rate_limit_name */
            "file_changes_rate_limit_status", /* scuba_status_name */
            hashmap! { "push_kind" => push_kind.as_str() },
        )
        .await
    }
    .map_err(|value| BundleResolverError::RateLimitExceeded {
        limit,
        entity: format!("{:?}", push_kind),
        value,
    })
}

pub(crate) async fn enforce_commit_rate_limits(
    ctx: &CoreContext,
    action: &PostResolveAction,
) -> Result<(), BundleResolverError> {
    let commits: Option<&_> = match action {
        PostResolveAction::Push(PostResolvePush {
            ref uploaded_bonsais,
            ..
        }) => Some(uploaded_bonsais),
        PostResolveAction::PushRebase(PostResolvePushRebase {
            ref uploaded_bonsais,
            ..
        }) => Some(uploaded_bonsais),
        // Currently, we do'nt rate-limit infinitepush:
        PostResolveAction::InfinitePush(..) => None,
        // This does not create any commits:
        PostResolveAction::BookmarkOnlyPushRebase(..) => None,
    };

    match commits {
        Some(ref commits) => enforce_commit_rate_limits_on_commits(ctx, commits.iter()).await,
        None => Ok(()),
    }
}

async fn enforce_commit_rate_limits_on_commits<'a, I: Iterator<Item = &'a BonsaiChangeset>>(
    ctx: &CoreContext,
    bonsais: I,
) -> Result<(), BundleResolverError> {
    let load_limiter = match ctx.session().load_limiter() {
        Some(load_limiter) => load_limiter,
        None => return Ok(()),
    };

    let category = load_limiter.category();
    let limit = &load_limiter.rate_limits().commits_per_author;

    let enforced = match limit.status {
        RateLimitStatus::Disabled => return Ok(()),
        RateLimitStatus::Tracked => false,
        RateLimitStatus::Enforced => true,
        // NOTE: Thrift enums aren't real enums once in Rust. We have to account for other values
        // here.
        _ => {
            let e = anyhow!("Invalid limit status: {:?}", limit.status);
            return Err(BundleResolverError::Error(e));
        }
    };

    let mut groups = HashMap::new();
    for bonsai in bonsais.into_iter() {
        *groups.entry(bonsai.author()).or_insert(0) += 1;
    }

    let counters = build_counters(ctx, category, limit, groups);
    let checks = dispatch_counter_checks_and_bumps(ctx, limit, counters, enforced);

    match timeout(
        Duration::from_secs(limit.timeout as u64),
        try_join_all(checks),
    )
    .await
    {
        Ok(Ok(_)) => Ok(()),
        Ok(Err((author, count))) => Err(BundleResolverError::RateLimitExceeded {
            limit: limit.clone(),
            entity: author,
            value: count as f64,
        }),
        Err(_) => {
            ctx.scuba()
                .clone()
                .log_with_msg("Rate Limit: Timed out", None);
            Ok(())
        }
    }
}

fn build_counters(
    ctx: &CoreContext,
    category: &str,
    limit: &RateLimit,
    groups: HashMap<&str, u64>,
) -> Vec<(BoxGlobalTimeWindowCounter, String, u64)> {
    groups
        .into_iter()
        .map(|(author, count)| {
            let key = make_key(&limit.prefix, author);
            debug!(
                ctx.logger(),
                "Associating key {:?} with author {:?}", key, author
            );

            let counter = GlobalTimeWindowCounterBuilder::build(
                ctx.fb,
                category,
                key,
                TIME_WINDOW_MIN,
                TIME_WINDOW_MAX,
            );
            (counter, author.to_owned(), count)
        })
        .collect()
}

fn dispatch_counter_checks_and_bumps<'a>(
    ctx: &'a CoreContext,
    limit: &'a RateLimit,
    counters: Vec<(BoxGlobalTimeWindowCounter, String, u64)>,
    enforced: bool,
) -> impl Iterator<Item = BoxFuture<'a, Result<(), (String, f64)>>> + 'a {
    let max_value = limit.max_value as f64;
    let interval = limit.interval as u32;
    let timeout_dur = Duration::from_secs(limit.timeout as u64);

    counters.into_iter().map(move |(counter, author, bump)| {
        async move {
            counter_check_and_bump(
                ctx,
                counter,
                max_value,
                interval,
                timeout_dur,
                bump as f64,
                enforced,
                "Commits Per Author Rate Limit",
                "commits_per_author_rate_limit_status",
                hashmap! {"author" => author.as_str() },
            )
            .await
            .map_err(|count| (author, count))
        }
        .boxed()
    })
}

/// Check if a counter would exceed maximum value if bumped
/// and bump it if it would not. If getting the counter
/// value times out, just act as if rate-limit check passes.
/// Returns
async fn counter_check_and_bump<'a>(
    ctx: &'a CoreContext,
    counter: BoxGlobalTimeWindowCounter,
    max_value: f64,
    interval: u32,
    timeout_dur: Duration,
    bump: f64,
    enforced: bool,
    rate_limit_name: &'a str,
    scuba_status_name: &'a str,
    scuba_extras: HashMap<&'a str, &'a str>,
) -> Result<(), f64> {
    let mut scuba = ctx.scuba().clone();
    for (key, val) in scuba_extras {
        scuba.add(key, val);
    }

    match timeout(timeout_dur, counter.get(interval)).await {
        Ok(Ok(count)) => {
            // NOTE: We only bump after we've allowed a response. This is reasonable for
            // this kind of limit.
            scuba.add(scuba_status_name, count);
            let new_value = count + bump;
            if new_value <= max_value {
                debug!(
                    ctx.logger(),
                    "Rate-limiting counter {} ({}) does not exceed threshold {} if bumped by {}",
                    rate_limit_name,
                    count,
                    max_value,
                    bump
                );
                let msg = format!("{}: Passed", rate_limit_name);
                scuba.log_with_msg(&msg, None);
                counter.bump(bump);
                Ok(())
            } else if !enforced {
                debug!(
                    ctx.logger(),
                    "Rate-limiting counter {} ({}) exceeds threshold {} if bumped by {}, but enforcement is disabled",
                    rate_limit_name,
                    count,
                    max_value,
                    bump
                );
                let msg = format!("{}: Skipped", rate_limit_name);
                scuba.log_with_msg(&msg, None);
                Ok(())
            } else {
                debug!(
                    ctx.logger(),
                    "Rate-limiting counter {} ({}) exceeds threshold {} if bumped by {}. Blocking request",
                    rate_limit_name,
                    count,
                    max_value,
                    bump
                );
                let msg = format!("{}: Blocked", rate_limit_name);
                scuba.log_with_msg(&msg, None);
                Err(new_value)
            }
        }
        Ok(Err(e)) => {
            debug!(
                ctx.logger(),
                "Failed getting rate limiting counter {}: {:?}", rate_limit_name, e
            );
            let msg = format!("{}: Failed", rate_limit_name);
            scuba.log_with_msg(&msg, None);
            Ok(())
        }
        Err(_) => {
            let msg = format!("{}: Timed out", rate_limit_name);
            scuba.log_with_msg(&msg, None);
            Ok(())
        }
    }
}

fn make_key(prefix: &str, author: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.input(author);
    let key = format!("{}.{}", prefix, hex::encode(hasher.result()));
    return key;
}
