/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use graphql_client::GraphQLQuery;
use serde::Serialize;

use crate::git_hub_repo::GitHubRepo;
use crate::make_request::make_request;

// See https://github.com/graphql-rust/graphql-client#custom-scalars
type GitObjectID = String;
#[allow(clippy::upper_case_acronyms)]
type URI = String;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.docs.graphql",
    query_path = "graphql/pull_request.graphql",
    deprecated = "warn"
)]
struct PullRequestQuery;

#[derive(Debug, Serialize)]
pub struct PullRequest {
    pub url: String,
    pub title: String,
    pub body: String,
    pub is_draft: bool,
    pub state: PullRequestState,
    pub closed: bool,
    pub merged: bool,
    pub review_decision: Option<PullRequestReviewDecision>,
    pub base: PullRequestRef,
    pub head: PullRequestRef,
}

#[derive(Debug, Serialize)]
pub struct PullRequestRef {
    pub ref_oid: String,
    pub ref_name: String,
    pub repo: GitHubRepo,
}

#[derive(Debug, Serialize)]
pub enum PullRequestState {
    Closed,
    Merged,
    Open,
    /// Catch-all in the event we get a value back we cannot resolve.
    Unknown,
}

impl From<pull_request_query::PullRequestState> for PullRequestState {
    fn from(state: pull_request_query::PullRequestState) -> Self {
        match state {
            pull_request_query::PullRequestState::CLOSED => PullRequestState::Closed,
            pull_request_query::PullRequestState::MERGED => PullRequestState::Merged,
            pull_request_query::PullRequestState::OPEN => PullRequestState::Open,
            pull_request_query::PullRequestState::Other(_) => PullRequestState::Unknown,
        }
    }
}

#[derive(Debug, Serialize)]
pub enum PullRequestReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
    /// Catch-all in the event we get a value back we cannot resolve.
    Unknown,
}

impl From<pull_request_query::PullRequestReviewDecision> for PullRequestReviewDecision {
    fn from(review_decision: pull_request_query::PullRequestReviewDecision) -> Self {
        match review_decision {
            pull_request_query::PullRequestReviewDecision::APPROVED => {
                PullRequestReviewDecision::Approved
            }
            pull_request_query::PullRequestReviewDecision::CHANGES_REQUESTED => {
                PullRequestReviewDecision::ChangesRequested
            }
            pull_request_query::PullRequestReviewDecision::REVIEW_REQUIRED => {
                PullRequestReviewDecision::ReviewRequired
            }
            pull_request_query::PullRequestReviewDecision::Other(_) => {
                PullRequestReviewDecision::Unknown
            }
        }
    }
}

pub fn get_pull_request(
    github_api_token: &str,
    repo: &GitHubRepo,
    number: u32,
) -> Result<Option<PullRequest>> {
    let pull_request_variables = pull_request_query::Variables {
        owner: repo.owner.to_string(),
        name: repo.name.to_string(),
        number: number.into(),
    };

    let query = PullRequestQuery::build_query(pull_request_variables);
    let response_data = make_request::<
        pull_request_query::ResponseData,
        pull_request_query::Variables,
    >(github_api_token, query)?;
    Ok(convert(response_data))
}

fn convert(response_data: pull_request_query::ResponseData) -> Option<PullRequest> {
    let repo = response_data.repository?;
    let pull_request = repo.pull_request?;
    let pull_request_query::PullRequestQueryRepositoryPullRequest {
        url,
        title,
        body,
        is_draft,
        state,
        closed,
        merged,
        review_decision,

        base_ref_name,
        base_ref_oid,
        base_repository,
        head_ref_name,
        head_ref_oid,
        head_repository,
    } = pull_request;
    let base_repository = base_repository?;
    let head_repository = head_repository?;

    let base_owner_name = base_repository.name_with_owner.split_once('/')?;
    let head_owner_name = head_repository.name_with_owner.split_once('/')?;
    Some(PullRequest {
        url,
        title,
        body,
        is_draft,
        state: PullRequestState::from(state),
        closed,
        merged,
        review_decision: review_decision.map(PullRequestReviewDecision::from),
        base: PullRequestRef {
            ref_oid: base_ref_oid,
            ref_name: base_ref_name,
            repo: GitHubRepo {
                owner: base_owner_name.0.to_string(),
                name: base_owner_name.1.to_string(),
            },
        },
        head: PullRequestRef {
            ref_oid: head_ref_oid,
            ref_name: head_ref_name,
            repo: GitHubRepo {
                owner: head_owner_name.0.to_string(),
                name: head_owner_name.1.to_string(),
            },
        },
    })
}
