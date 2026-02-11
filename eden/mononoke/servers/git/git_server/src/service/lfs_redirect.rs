/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! LFS redirect handler for the Mononoke Git server.
//!
//! By default git-lfs infers the LFS server's URL by just reusing the repo's remote url.
//! This will result in a request made to the git server. Here we're handling this endpoint,
//! which allows us to tell clients what is the authoritative LFS server as far as this git server
//! is concerned. We're taking control over this server-side. We do this over a HTTP redirect.

use std::pin::Pin;

use futures::FutureExt;
use futures::StreamExt;
use gotham::handler::HandlerFuture;
use gotham::helpers::http::response::create_temporary_redirect;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::error::HttpError;
use gotham_ext::middleware::ScubaMiddlewareState;
use gotham_ext::response::build_error_response;
use hyper::Body;
use hyper::Response;
use hyper::header::HOST;
use repourl::encode_repo_name;
use stats::prelude::*;

use super::error_formatter::GitErrorFormatter;
use crate::model::RepositoryParams;

define_stats! {
    prefix = "mononoke.git.server.lfs_redirect";
    redirect_issued: timeseries(Rate, Sum),
    redirect_error: timeseries(Rate, Sum),
}

const ROUTE_TO_MONONOKE_GIT_LFS: &str = "x-route-to-mononoke-git-lfs";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NetworkEnv {
    Prod,
    Corp,
    CorpX2pagent,
    X2P,
}

impl NetworkEnv {
    fn from_host_header(host: &str, corpx2pagent: bool) -> Option<Self> {
        match (host, corpx2pagent) {
            ("git.c2p.facebook.net", true) => Some(NetworkEnv::CorpX2pagent),
            ("git.c2p.facebook.net", _) => Some(NetworkEnv::Corp),
            ("git.edge.x2p.facebook.net", _) => Some(NetworkEnv::X2P),
            ("git.internal.tfbnw.net", _) => Some(NetworkEnv::Prod),
            _ => None,
        }
    }

    fn mononoke_lfs_url_prefix(&self) -> &'static str {
        match self {
            NetworkEnv::Prod => "https://mononoke-git-lfs.internal.tfbnw.net",
            NetworkEnv::Corp => "https://mononoke-git-lfs.c2p.facebook.net",
            NetworkEnv::X2P => "http://mononoke-git-lfs.edge.x2p.facebook.net",
            NetworkEnv::CorpX2pagent => "http://mononoke-git-lfs.c2p.facebook.net",
        }
    }
}

fn build_redirect_url(network_env: NetworkEnv, repo_name: &str, to_mononoke: bool) -> String {
    if to_mononoke {
        let encoded_repo = encode_repo_name(repo_name);
        format!(
            "{}/{}/objects/batch",
            network_env.mononoke_lfs_url_prefix(),
            encoded_repo
        )
    } else {
        format!(
            "https://dewey-lfs.vip.facebook.com/lfs-by-repo/{}/objects/batch",
            repo_name
        )
    }
}

fn read_header_value(state: &State, name: &str) -> Option<String> {
    let headers = hyper::HeaderMap::borrow_from(state);
    headers
        .get(name)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
}

fn should_route_to_mononoke_lfs(state: &State, repo_name: &str) -> bool {
    if let Some(header_value) = read_header_value(state, ROUTE_TO_MONONOKE_GIT_LFS) {
        match header_value.as_str() {
            "1" => return true,
            "0" => return false,
            _ => {}
        }
    }

    justknobs::eval("scm/metagit:mononoke_git_lfs", None, Some(repo_name)).unwrap_or(false)
}

async fn handle_lfs_redirect(state: &mut State) -> Result<Response<Body>, HttpError> {
    // Consume the request body to prevent connection drops. The LFS batch API sends
    // a JSON body with the list of objects, but we don't need it for routing decisions.
    // Failing to consume the body can cause the client connection to be dropped.
    if let Some(mut body) = Body::try_take_from(state) {
        while body.next().await.is_some() {}
    }

    let repo_name = RepositoryParams::borrow_from(state).repo_name();

    ScubaMiddlewareState::try_borrow_add(state, "method", "lfs_redirect");
    ScubaMiddlewareState::try_borrow_add(state, "repo", repo_name.as_str());

    let host_header = read_header_value(state, HOST.as_str())
        .ok_or_else(|| HttpError::e400(anyhow::anyhow!("No 'Host' header")))?;

    let corpx2pagent = read_header_value(state, "corpx2pagent").is_some();

    let network_env =
        NetworkEnv::from_host_header(&host_header, corpx2pagent).ok_or_else(|| {
            HttpError::e400(anyhow::anyhow!(
                "Hostname in the 'Host' header not supported: {}",
                host_header
            ))
        })?;

    let to_mononoke = should_route_to_mononoke_lfs(state, &repo_name);
    let redirect_url = build_redirect_url(network_env, &repo_name, to_mononoke);

    ScubaMiddlewareState::try_borrow_add(state, "lfs_redirect_url", redirect_url.as_str());
    ScubaMiddlewareState::try_borrow_add(
        state,
        "lfs_to_mononoke",
        if to_mononoke { "true" } else { "false" },
    );

    STATS::redirect_issued.add_value(1);

    Ok(create_temporary_redirect(state, redirect_url))
}

pub fn lfs_redirect_handler(mut state: State) -> Pin<Box<HandlerFuture>> {
    async move {
        match handle_lfs_redirect(&mut state).await {
            Ok(res) => Ok((state, res)),
            Err(err) => {
                STATS::redirect_error.add_value(1);
                build_error_response(err, state, &GitErrorFormatter)
            }
        }
    }
    .boxed()
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_encode_repo_name_simple() {
        assert_eq!(encode_repo_name("simple-repo"), "simple-repo");
    }

    #[mononoke::test]
    fn test_encode_repo_name_with_slash() {
        assert_eq!(encode_repo_name("path/to/repo"), "path%2Fto%2Frepo");
    }

    #[mononoke::test]
    fn test_encode_repo_name_with_special_chars() {
        assert_eq!(
            encode_repo_name("repo-with+special chars"),
            "repo-with%2Bspecial%20chars"
        );
    }

    #[mononoke::test]
    fn test_encode_repo_name_nested_path() {
        assert_eq!(encode_repo_name("org/team/project"), "org%2Fteam%2Fproject");
    }

    #[mononoke::test]
    fn test_build_redirect_url_mononoke_prod() {
        let url = build_redirect_url(NetworkEnv::Prod, "test/repo", true);
        assert_eq!(
            url,
            "https://mononoke-git-lfs.internal.tfbnw.net/test%2Frepo/objects/batch"
        );
    }

    #[mononoke::test]
    fn test_build_redirect_url_mononoke_corp() {
        let url = build_redirect_url(NetworkEnv::Corp, "my/repo", true);
        assert_eq!(
            url,
            "https://mononoke-git-lfs.c2p.facebook.net/my%2Frepo/objects/batch"
        );
    }

    #[mononoke::test]
    fn test_build_redirect_url_mononoke_x2p() {
        let url = build_redirect_url(NetworkEnv::X2P, "another/repo", true);
        assert_eq!(
            url,
            "http://mononoke-git-lfs.edge.x2p.facebook.net/another%2Frepo/objects/batch"
        );
    }

    #[mononoke::test]
    fn test_build_redirect_url_mononoke_corpx2pagent() {
        let url = build_redirect_url(NetworkEnv::CorpX2pagent, "x2p/agent/repo", true);
        assert_eq!(
            url,
            "http://mononoke-git-lfs.c2p.facebook.net/x2p%2Fagent%2Frepo/objects/batch"
        );
    }

    #[mononoke::test]
    fn test_build_redirect_url_dewey() {
        let url = build_redirect_url(NetworkEnv::Prod, "test/repo", false);
        assert_eq!(
            url,
            "https://dewey-lfs.vip.facebook.com/lfs-by-repo/test/repo/objects/batch"
        );
    }

    #[mononoke::test]
    fn test_build_redirect_url_dewey_special_chars() {
        let url = build_redirect_url(NetworkEnv::Corp, "test/repo-with+special", false);
        assert_eq!(
            url,
            "https://dewey-lfs.vip.facebook.com/lfs-by-repo/test/repo-with+special/objects/batch"
        );
    }

    #[mononoke::test]
    fn test_network_env_from_header_prod() {
        assert_eq!(
            NetworkEnv::from_host_header("git.internal.tfbnw.net", false),
            Some(NetworkEnv::Prod)
        );
    }

    #[mononoke::test]
    fn test_network_env_from_header_corp() {
        assert_eq!(
            NetworkEnv::from_host_header("git.c2p.facebook.net", false),
            Some(NetworkEnv::Corp)
        );
    }

    #[mononoke::test]
    fn test_network_env_from_header_corpx2pagent() {
        assert_eq!(
            NetworkEnv::from_host_header("git.c2p.facebook.net", true),
            Some(NetworkEnv::CorpX2pagent)
        );
    }

    #[mononoke::test]
    fn test_network_env_from_header_x2p() {
        assert_eq!(
            NetworkEnv::from_host_header("git.edge.x2p.facebook.net", false),
            Some(NetworkEnv::X2P)
        );
    }

    #[mononoke::test]
    fn test_network_env_from_header_unknown() {
        assert!(NetworkEnv::from_host_header("unknown.host.com", false).is_none());
    }
}
