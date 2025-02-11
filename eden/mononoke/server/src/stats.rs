/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use context::CoreContext;
use mononoke_api::Repo;
use mononoke_api::RepositoryId;
use phases::PhasesRef;
use sql_commit_graph_storage::CommitGraphBulkFetcherRef;
use stats::define_stats;
use stats::prelude::*;

const STATS_LOOP_INTERNAL: Duration = Duration::from_secs(5 * 60);

define_stats! {
    prefix = "mononoke.repo.stats";
    all_commit_count: dynamic_singleton_counter("all_commit_count.{}", (repo: String)),
    public_commit_count: dynamic_singleton_counter("public_commit_count.{}", (repo: String)),
    draft_commit_count: dynamic_singleton_counter("draft_commit_count.{}", (repo: String)),
}

pub(crate) async fn stats_loop(
    ctx: CoreContext,
    repo_name: String,
    repo_id: RepositoryId,
    repo: Arc<Repo>,
) {
    loop {
        let all_commits = match repo
            .commit_graph_bulk_fetcher()
            .fetch_commit_count(&ctx, repo_id)
            .await
        {
            Ok(count) => count,
            Err(err) => {
                eprintln!("Error fetching all commits: {}", err);
                continue;
            }
        };
        let public = match repo.phases().count_all_public(&ctx, repo_id).await {
            Ok(count) => count,
            Err(err) => {
                eprintln!("Error counting public commits: {}", err);
                continue;
            }
        };

        STATS::all_commit_count.set_value(ctx.fb, all_commits as i64, (repo_name.to_string(),));
        STATS::public_commit_count.set_value(ctx.fb, public as i64, (repo_name.to_string(),));
        STATS::draft_commit_count.set_value(
            ctx.fb,
            (all_commits - public) as i64,
            (repo_name.to_string(),),
        );

        tokio::time::sleep(STATS_LOOP_INTERNAL).await;
    }
}
