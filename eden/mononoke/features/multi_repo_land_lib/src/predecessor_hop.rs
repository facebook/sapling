/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Resolves a stacked land's off-branch baseline one hop back through the
//! git client's `predecessor` headers, over commits the land service itself
//! recorded in the pushrebase mutation mapping.

use std::collections::HashSet;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Loadable;
use blobstore::LoadableError;
use bonsai_git_mapping::BonsaiGitMappingRef;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use git_types::mutation::extract_mutation_from_headers;
use mononoke_types::ChangesetId;
use pushrebase::BaselineCandidateSource;
use pushrebase::range_ids_above_capped;
use pushrebase_mutation_mapping::PushrebaseMutationMappingRef;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;

/// Branch commits since the fork point whose recorded predecessors are read.
pub const PREDECESSOR_HOP_MAX_BRANCH_COMMITS: usize = 200;
const PREDECESSOR_HOP_CONCURRENCY: usize = 10;

/// A recorded predecessor whose `predecessor` header names the baseline,
/// written by a `rebase` or `amend` of the baseline's author, offers its
/// recorded successor as the baseline's landed form.
pub struct RecordedPredecessorHop<'a, R> {
    repo: &'a R,
}

impl<'a, R> RecordedPredecessorHop<'a, R> {
    pub fn new(repo: &'a R) -> Self {
        Self { repo }
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct HopMatch {
    predecessor: ChangesetId,
    landed: ChangesetId,
    op: String,
    author_matches: bool,
}

impl HopMatch {
    fn eligible(&self) -> bool {
        self.author_matches && matches!(self.op.as_str(), "rebase" | "amend")
    }
}

#[async_trait]
impl<R> BaselineCandidateSource for RecordedPredecessorHop<'_, R>
where
    R: CommitGraphRef
        + RepoBlobstoreRef
        + PushrebaseMutationMappingRef
        + BonsaiGitMappingRef
        + RepoIdentityRef
        + Send
        + Sync,
{
    fn name(&self) -> &'static str {
        "predecessor_hop"
    }

    async fn candidates_for(
        &self,
        ctx: &CoreContext,
        baseline: ChangesetId,
        onto: ChangesetId,
        fork_point: ChangesetId,
    ) -> Result<Vec<ChangesetId>> {
        let repo = self.repo;
        let mut scuba = ctx.scuba().clone();
        scuba
            .add("log_tag", "predecessor_hop")
            .add("repo", repo.repo_identity().name())
            .add("old_target", baseline.to_string())
            .add("onto", onto.to_string());
        let Some(baseline_git) = repo
            .bonsai_git_mapping()
            .get_git_sha1_from_bonsai(ctx, baseline)
            .await?
        else {
            scuba
                .add("outcome", "baseline_has_no_git_sha")
                .unsampled()
                .log();
            return Ok(Vec::new());
        };
        let Some(branch) = range_ids_above_capped(
            ctx,
            repo,
            fork_point,
            onto,
            PREDECESSOR_HOP_MAX_BRANCH_COMMITS,
        )
        .await?
        else {
            scuba.add("outcome", "branch_too_long").unsampled().log();
            return Ok(Vec::new());
        };
        scuba.add("branch_commits", branch.len());
        let baseline_author = baseline
            .load(ctx, repo.repo_blobstore())
            .await?
            .author()
            .to_string();
        let baseline_author = &baseline_author;

        let mut matches: Vec<HopMatch> =
            stream::iter(branch.into_iter().map(|landed| async move {
                let predecessors: HashSet<ChangesetId> = repo
                    .pushrebase_mutation_mapping()
                    .get_prepushrebase_ids(ctx, landed)
                    .await?
                    .into_iter()
                    .collect();
                let mut found = Vec::new();
                for predecessor in predecessors {
                    let bcs = match predecessor.load(ctx, repo.repo_blobstore()).await {
                        Ok(bcs) => bcs,
                        Err(LoadableError::Missing(_)) => continue,
                        Err(err) => return Err(anyhow::Error::from(err)),
                    };
                    let headers: Vec<(&[u8], &[u8])> =
                        bcs.git_extra_headers().into_iter().flatten().collect();
                    let Some(mutation) = extract_mutation_from_headers(&headers) else {
                        continue;
                    };
                    if !mutation.predecessors.contains(&baseline_git) {
                        continue;
                    }
                    found.push(HopMatch {
                        predecessor,
                        landed,
                        op: mutation.op,
                        author_matches: bcs.author() == baseline_author,
                    });
                }
                anyhow::Ok(found)
            }))
            .buffered(PREDECESSOR_HOP_CONCURRENCY)
            .try_concat()
            .await?;
        matches.sort();
        matches.dedup();

        let eligible: Vec<&HopMatch> = matches.iter().filter(|m| m.eligible()).collect();
        scuba
            .add("matches", matches.len())
            .add("eligible", eligible.len())
            .add(
                "predecessors",
                eligible
                    .iter()
                    .map(|m| format!("{}:{}", m.predecessor, m.op))
                    .collect::<Vec<_>>()
                    .join(","),
            )
            .add(
                "outcome",
                match eligible.len() {
                    0 => "none",
                    1 => "candidate",
                    _ => "ambiguous",
                },
            )
            .unsampled()
            .log();
        Ok(eligible.into_iter().map(|m| m.landed).collect())
    }
}

/// Names a commit for a client message: git SHA when mapped, else the
/// bonsai id, plus the `Differential Revision` from its message.
pub async fn describe_commit_for_client(
    ctx: &CoreContext,
    repo: &(impl BonsaiGitMappingRef + RepoBlobstoreRef),
    cs_id: ChangesetId,
) -> String {
    let mut label = match repo
        .bonsai_git_mapping()
        .get_git_sha1_from_bonsai(ctx, cs_id)
        .await
    {
        Ok(Some(git)) => git.to_hex().to_string(),
        _ => cs_id.to_string(),
    };
    if let Ok(bcs) = cs_id.load(ctx, repo.repo_blobstore()).await {
        if let Some(revision) = differential_revision(bcs.message()) {
            label.push_str(&format!(" ({revision})"));
        }
    }
    label
}

/// `D12345` from a `Differential Revision:` trailer, if any.
pub fn differential_revision(message: &str) -> Option<String> {
    message
        .lines()
        .find_map(|line| line.trim().strip_prefix("Differential Revision:"))
        .and_then(|rest| rest.trim().rsplit('/').next())
        .filter(|revision| !revision.is_empty())
        .map(str::to_string)
}
