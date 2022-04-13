/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use graphql_client::QueryBody;
use graphql_client::Response as GraphQLResponse;
use http_client::Method;
use http_client::Request;
use serde::de::DeserializeOwned;
use serde::Serialize;
use url::Url;

pub(crate) fn make_request<D: DeserializeOwned, V: Serialize>(
    github_api_token: &str,
    query: QueryBody<V>,
) -> Result<D> {
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

    match response.json::<GraphQLResponse<D>>()?.data {
        Some(response_data) => Ok(response_data),
        None => Err(anyhow!(
            "failed to parse '{}'",
            String::from_utf8_lossy(response.body())
        )),
    }
}
