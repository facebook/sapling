/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use graphql_client::GraphQLQuery;
use graphql_client::Response as GraphQLResponse;
use http_client::Method;
use http_client::Request;
use url::Url;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.docs.graphql",
    query_path = "graphql/queries.graphql",
    deprecated = "warn"
)]
struct RepoParent;

pub struct GitHubRepo {
    pub owner: String,
    pub name: String,
}

pub fn get_parent_repo(github_api_token: &str, repo: &GitHubRepo) -> Result<Option<GitHubRepo>> {
    let repo_parent_variables = repo_parent::Variables {
        owner: repo.owner.to_string(),
        name: repo.name.to_string(),
    };

    let query = RepoParent::build_query(repo_parent_variables);

    let url = Url::parse("https://api.github.com/graphql")?;
    let mut request = Request::new(url, Method::Post);
    request.set_header("authorization", format!("Bearer {}", github_api_token));
    // If User-Agent is not set, the request will fail with a 403:
    // "Request forbidden by administrative rules. Please make sure your request
    // has a User-Agent header".
    request.set_header("User-Agent", "graphql-rust/0.10.0");
    request.set_body(serde_json::to_vec(&query)?);

    let response = request.send()?;
    let status = response.status();
    if !status.is_success() {
        bail!(
            "request failed ({}): {}",
            status.as_u16(),
            String::from_utf8_lossy(response.body())
        );
    }

    match response
        .json::<GraphQLResponse<repo_parent::ResponseData>>()?
        .data
    {
        Some(response_data) => Ok(convert(response_data)),
        None => Err(anyhow!(
            "failed to parse '{}'",
            String::from_utf8_lossy(response.body())
        )),
    }
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
