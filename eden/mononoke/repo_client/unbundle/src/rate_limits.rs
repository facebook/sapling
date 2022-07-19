/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::BundleResolverError;
use crate::PostResolveAction;
use crate::PostResolvePush;
use crate::PostResolvePushRebase;
use anyhow::anyhow;
use anyhow::Result;
use context::CoreContext;
use futures::future::try_join_all;
use futures::future::BoxFuture;
use futures::FutureExt;
use maplit::hashmap;
use mercurial_revlog::changeset::RevlogChangeset;
use mononoke_types::BonsaiChangeset;
use rate_limiting::RateLimitBody;
use rate_limiting::RateLimitStatus;
use sha2::Digest;
use sha2::Sha256;
use slog::debug;
use slog::warn;
use std::collections::HashMap;
use std::time::Duration;
use time_window_counter::BoxGlobalTimeWindowCounter;
use time_window_counter::GlobalTimeWindowCounterBuilder;
use tokio::time::timeout;

const TIME_WINDOW_MIN: u32 = 10;
const TIME_WINDOW_MAX: u32 = 3600;
const RATELIM_FETCH_TIMEOUT: Duration = Duration::from_secs(1);

const COMMITS_PER_AUTHOR_KEY: &str = "commits_per_author";
const COMMITS_PER_AUTHOR_LIMIT_NAME: &str = "Commits Per Author Rate Limit";
const TOTAL_FILE_CHANGES_KEY: &str = "total_file_changes";
const TOTAL_FILE_CHANGES_LIMIT_NAME: &str = "File Changes Rate Limit";

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

fn get_file_changes_rate_limit(ctx: &CoreContext) -> Option<(RateLimitBody, &str)> {
    let maybe_rate_limit_with_category = ctx.session().rate_limiter().and_then(|rate_limiter| {
        let category = rate_limiter.category();
        rate_limiter
            .total_file_changes_limit()
            .map(|limit| (limit, category))
    });

    if maybe_rate_limit_with_category.is_none() {
        debug!(ctx.logger(), "file-changes rate limit not enabled");
    }

    maybe_rate_limit_with_category
}

pub(crate) async fn enforce_file_changes_rate_limits<
    'a,
    RC: Iterator<Item = &'a RevlogChangeset>,
>(
    ctx: &CoreContext,
    push_kind: RateLimitedPushKind,
    revlog_changesets: RC,
) -> Result<(), BundleResolverError> {
    let (limit, category) = match get_file_changes_rate_limit(ctx) {
        Some((limit, category)) => (limit, category),
        None => return Ok(()),
    };

    let enforced = match limit.raw_config.status {
        RateLimitStatus::Disabled => return Ok(()),
        RateLimitStatus::Tracked => false,
        RateLimitStatus::Enforced => true,
        // NOTE: Thrift enums aren't real enums once in Rust. We have to account for other values
        // here.
        _ => {
            let e = anyhow!(
                "Invalid file count rate limit status: {:?}",
                limit.raw_config.status
            );
            return Err(BundleResolverError::Error(e));
        }
    };

    let max_value = limit.raw_config.limit as f64;
    let interval = limit.window.as_secs() as u32;

    let counter = GlobalTimeWindowCounterBuilder::build(
        ctx.fb,
        category,
        TOTAL_FILE_CHANGES_KEY,
        TIME_WINDOW_MIN,
        TIME_WINDOW_MAX,
    );
    let total_file_number: usize = revlog_changesets.map(|rc| rc.files().len()).sum();
    let res = {
        let push_kind = format!("{}", push_kind);
        counter_check_and_bump(
            ctx,
            counter,
            max_value,
            interval,
            total_file_number as f64,
            enforced,
            TOTAL_FILE_CHANGES_LIMIT_NAME,
            "file_changes_rate_limit_status", /* scuba_status_name */
            hashmap! { "push_kind" => push_kind.as_str() },
        )
        .await
    }
    .map_err(|value| BundleResolverError::RateLimitExceeded {
        limit_name: TOTAL_FILE_CHANGES_LIMIT_NAME.to_string(),
        limit,
        entity: format!("{:?}", push_kind),
        value,
    });

    if push_kind == RateLimitedPushKind::Public && res.is_err() {
        // For public pushes, let's bump the counter,
        // but never actually block them
        warn!(
            ctx.logger(),
            "File Changes Rate Limit enforced and exceeded, but push is allowed, as it is public"
        );
        Ok(())
    } else {
        res
    }
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
        Some(commits) => enforce_commit_rate_limits_on_commits(ctx, commits.iter()).await,
        None => Ok(()),
    }
}

async fn enforce_commit_rate_limits_on_commits<'a, I: Iterator<Item = &'a BonsaiChangeset>>(
    ctx: &CoreContext,
    bonsais: I,
) -> Result<(), BundleResolverError> {
    let rate_limiter = match ctx.session().rate_limiter() {
        Some(rate_limiter) => rate_limiter,
        None => return Ok(()),
    };

    let category = rate_limiter.category();

    let limit = match rate_limiter.commits_per_author_limit() {
        Some(limit) => limit,
        None => return Ok(()),
    };

    let enforced = match limit.raw_config.status {
        RateLimitStatus::Disabled => return Ok(()),
        RateLimitStatus::Tracked => false,
        RateLimitStatus::Enforced => true,
        // NOTE: Thrift enums aren't real enums once in Rust. We have to account for other values
        // here.
        _ => {
            let e = anyhow!("Invalid limit status: {:?}", limit.raw_config.status);
            return Err(BundleResolverError::Error(e));
        }
    };

    let mut groups = HashMap::new();
    for bonsai in bonsais {
        *groups.entry(bonsai.author()).or_insert(0) += 1;
    }

    let counters = build_counters(ctx, category, groups);
    let checks = dispatch_counter_checks_and_bumps(ctx, &limit, counters, enforced);

    match timeout(RATELIM_FETCH_TIMEOUT, try_join_all(checks)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err((author, count))) => Err(BundleResolverError::RateLimitExceeded {
            limit_name: COMMITS_PER_AUTHOR_LIMIT_NAME.to_string(),
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
    groups: HashMap<&str, u64>,
) -> Vec<(BoxGlobalTimeWindowCounter, String, u64)> {
    groups
        .into_iter()
        .map(|(author, count)| {
            let key = make_key(COMMITS_PER_AUTHOR_KEY, author);
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
    limit: &'a RateLimitBody,
    counters: Vec<(BoxGlobalTimeWindowCounter, String, u64)>,
    enforced: bool,
) -> impl Iterator<Item = BoxFuture<'a, Result<(), (String, f64)>>> + 'a {
    let max_value = limit.raw_config.limit as f64;
    let interval = limit.window.as_secs() as u32;

    counters.into_iter().map(move |(counter, author, bump)| {
        async move {
            counter_check_and_bump(
                ctx,
                counter,
                max_value,
                interval,
                bump as f64,
                enforced,
                COMMITS_PER_AUTHOR_LIMIT_NAME,
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

    match timeout(RATELIM_FETCH_TIMEOUT, counter.get(interval)).await {
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
    hasher.update(author);
    let key = format!("{}.{}", prefix, hex::encode(hasher.finalize()));
    key
}
