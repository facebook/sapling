/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use graphql_client::GraphQLQuery;

use crate::git_hub_repo::GitHubRepo;
use crate::make_request::make_request;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.docs.graphql",
    query_path = "graphql/queries.graphql",
    deprecated = "warn"
)]
struct RepoParent;

pub fn get_parent_repo(github_api_token: &str, repo: &GitHubRepo) -> Result<Option<GitHubRepo>> {
    let repo_parent_variables = repo_parent::Variables {
        owner: repo.owner.to_string(),
        name: repo.name.to_string(),
    };

    let query = RepoParent::build_query(repo_parent_variables);
    let response_data =
        make_request::<repo_parent::ResponseData, repo_parent::Variables>(github_api_token, query)?;
    Ok(convert(response_data))
}

fn convert(response_data: repo_parent::ResponseData) -> Option<GitHubRepo> {
    let repo = response_data.repository?;
    if repo.is_fork {
        let parent = repo.parent.expect("is_fork is true, so parent must be set");
        Some(GitHubRepo {
            owner: parent.owner.login,
            name: parent.name,
        })
    } else {
        None
    }
}
