/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use bundle2_resolver::{
    BundleResolverError, PostResolveAction, PostResolvePush, PostResolvePushRebase,
};
use cloned::cloned;
use context::CoreContext;
use crypto::{digest::Digest, sha2::Sha256};
use failure_ext::format_err;
use futures::{future::join_all, Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use limits::types::{RateLimit, RateLimitStatus};
use mononoke_types::BonsaiChangeset;
use ratelim::time_window_counter::TimeWindowCounter;
use scuba_ext::ScubaSampleBuilderExt;
use slog::debug;
use std::collections::HashMap;
use std::time::Duration;
use tokio::util::FutureExt as TokioFutureExt;

const TIME_WINDOW_MIN: u32 = 10;
const TIME_WINDOW_MAX: u32 = 3600;

pub fn enforce_commit_rate_limits<'a>(
    ctx: CoreContext,
    action: &'a PostResolveAction,
) -> impl Future<Item = (), Error = BundleResolverError> + 'static {
    let commits: Option<&'a _> = match action {
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
        Some(ref commits) => {
            enforce_commit_rate_limits_on_commits(ctx, commits.iter()).left_future()
        }
        None => Ok(()).into_future().right_future(),
    }
}

fn enforce_commit_rate_limits_on_commits<'a, I: Iterator<Item = &'a BonsaiChangeset>>(
    ctx: CoreContext,
    bonsais: I,
) -> BoxFuture<(), BundleResolverError> {
    let load_limiter = match ctx.session().load_limiter() {
        Some(load_limiter) => load_limiter,
        None => return Ok(()).into_future().boxify(),
    };

    let category = load_limiter.category();
    let limit = load_limiter.rate_limits().commits_per_author.clone();

    let enforced = match limit.status {
        RateLimitStatus::Disabled => return Ok(()).into_future().boxify(),
        RateLimitStatus::Tracked => false,
        RateLimitStatus::Enforced => true,
        // NOTE: Thrift enums aren't real enums once in Rust. We have to account for other values
        // here.
        _ => {
            let e = format_err!("Invalid limit status: {:?}", limit.status);
            return Err(BundleResolverError::Error(e)).into_future().boxify();
        }
    };

    let mut groups = HashMap::new();
    for bonsai in bonsais.into_iter() {
        *groups.entry(bonsai.author()).or_insert(0) += 1;
    }

    let counters = build_counters(&ctx, &category, &limit, groups);
    let checks = dispatch_counter_checks_and_bumps(ctx.clone(), &limit, counters, enforced);

    join_all(checks)
        .map(|_| ())
        .timeout(Duration::from_secs(limit.timeout as u64))
        .or_else(move |err| match err.into_inner() {
            Some((author, count)) => Err(BundleResolverError::RateLimitExceeded {
                limit,
                entity: author,
                value: count as f64,
            }),
            // into_inner() being None means we had a timeout. We fail open in this case.
            None => {
                ctx.scuba()
                    .clone()
                    .log_with_msg("Rate Limit: Timed out", None);
                Ok(())
            }
        })
        .boxify()
}

fn build_counters(
    ctx: &CoreContext,
    category: &str,
    limit: &RateLimit,
    groups: HashMap<&str, u64>,
) -> Vec<(TimeWindowCounter, String, u64)> {
    groups
        .into_iter()
        .map(|(author, count)| {
            let key = make_key(&limit.prefix, author);
            debug!(
                ctx.logger(),
                "Associating key {:?} with author {:?}", key, author
            );

            let counter =
                TimeWindowCounter::new(ctx.fb, category, key, TIME_WINDOW_MIN, TIME_WINDOW_MAX);
            (counter, author.to_owned(), count)
        })
        .collect()
}

fn dispatch_counter_checks_and_bumps(
    ctx: CoreContext,
    limit: &RateLimit,
    counters: Vec<(TimeWindowCounter, String, u64)>,
    enforced: bool,
) -> Vec<BoxFuture<(), (String, f64)>> {
    let max_value = limit.max_value as f64;
    let interval = limit.interval as u32;

    counters
        .into_iter()
        .map(move |(counter, author, bump)| {
            cloned!(ctx);
            counter
                .get(interval)
                .then(move |res| {
                    // NOTE: We only bump after we've allowed a response. This is reasonable for
                    // this kind of limit.
                    let mut scuba = ctx.scuba().clone();
                    scuba.add("author", author.clone());

                    match res {
                        Ok(count) => {
                            scuba.add("rate_limit_status", count);

                            if count <= max_value {
                                scuba.log_with_msg("Rate Limit: Passed", None);
                                counter.bump(bump as f64);
                                Ok(())
                            } else if !enforced {
                                scuba.log_with_msg("Rate Limit: Skipped", None);
                                Ok(())
                            } else {
                                scuba.log_with_msg("Rate Limit: Blocked", None);
                                Err((author, count))
                            }
                        }
                        // Fail open if we fail to load something.
                        Err(_) => {
                            scuba.log_with_msg("Rate Limit: Failed", None);
                            Ok(())
                        }
                    }
                })
                .boxify()
        })
        .collect()
}

fn make_key(prefix: &str, author: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.input(author.as_ref());
    format!("{}.{}", prefix, hasher.result_str())
}
