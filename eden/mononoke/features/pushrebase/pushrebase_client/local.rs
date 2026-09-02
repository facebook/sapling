/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use bookmarks::BookmarkKey;
use bookmarks_movement::BookmarkKindRestrictions;
use bookmarks_movement::BookmarkMovementError;
use bookmarks_movement::Repo;
use bookmarks_movement::postprocess_pushrebase_outcome;
use bookmarks_movement::prepare_pushrebase_hooks;
use bookmarks_movement::pushrebase_flags;
use bookmarks_movement::validate_pushrebase_request;
use bytes::Bytes;
use context::CoreContext;
use futures_stats::TimedFutureExt;
use hooks::CrossRepoPushSource;
use hooks::HookManager;
use mononoke_types::BonsaiChangeset;
use pushrebase::PushrebaseOutcome;
use repo_authorization::AuthorizationContext;

use crate::PushrebaseClient;

pub struct LocalPushrebaseClient<'a, R: Repo> {
    pub ctx: &'a CoreContext,
    pub authz: &'a AuthorizationContext,
    pub repo: &'a R,
    pub hook_manager: &'a HookManager,
}

#[async_trait::async_trait]
impl<'a, R: Repo> PushrebaseClient for LocalPushrebaseClient<'a, R> {
    async fn pushrebase(
        &self,
        bookmark: &BookmarkKey,
        changesets: &[BonsaiChangeset],
        pushvars: Option<&HashMap<String, Bytes>>,
        cross_repo_push_source: CrossRepoPushSource,
        bookmark_restrictions: BookmarkKindRestrictions,
        log_new_public_commits_to_scribe: bool,
    ) -> Result<PushrebaseOutcome, BookmarkMovementError> {
        let kind = validate_pushrebase_request(
            self.ctx,
            self.authz,
            self.repo,
            self.hook_manager,
            bookmark,
            changesets,
            pushvars,
            cross_repo_push_source,
            bookmark_restrictions,
        )
        .await?;
        let hooks =
            prepare_pushrebase_hooks(self.ctx, self.authz, self.repo, bookmark, pushvars, kind)
                .await?;
        let source_changesets: HashSet<_> = changesets.iter().cloned().collect();

        self.ctx
            .scuba()
            .clone()
            .add("bookmark", bookmark.to_string())
            .log_with_msg("Pushrebase started", None);
        let (stats, result) = pushrebase::do_pushrebase_bonsai(
            self.ctx,
            self.repo,
            &pushrebase_flags(self.repo, bookmark, pushvars),
            bookmark,
            &source_changesets,
            &hooks,
        )
        .timed()
        .await;

        let mut scuba = self.ctx.scuba().clone();
        scuba.add_future_stats(&stats);
        match &result {
            Ok(outcome) => {
                scuba
                    .add("pushrebase_retry_num", outcome.retry_num.0)
                    .add("pushrebase_distance", outcome.pushrebase_distance.0)
                    .add("bookmark", bookmark.to_string())
                    .add("changeset_id", outcome.head.to_string());
                if let Some(paths) = &outcome.merge_resolved_paths {
                    scuba.add("merge_resolved_count", paths.len()).add(
                        "merge_resolved_paths",
                        paths
                            .iter()
                            .take(10)
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                }
                scuba.log_with_msg("Pushrebase finished", None);

                postprocess_pushrebase_outcome(
                    self.ctx,
                    self.repo,
                    bookmark,
                    kind,
                    outcome,
                    &source_changesets,
                    log_new_public_commits_to_scribe,
                )
                .await?;
            }
            Err(err) => {
                if let pushrebase::PushrebaseError::Conflicts(conflicts) = err {
                    scuba.add("conflict_count", conflicts.len()).add(
                        "conflict_paths",
                        conflicts
                            .iter()
                            .take(10)
                            .map(|conflict| format!("{}={}", conflict.left, conflict.right))
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                }
                scuba.log_with_msg("Pushrebase failed", Some(format!("{err:#?}")));
            }
        }

        result.map_err(BookmarkMovementError::PushrebaseError)
    }
}
