/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::error::HttpError;
use gotham_ext::middleware::request_context::RequestContext;
use hyper::Body;
use hyper::Response;
use memory_profiling::check_acl_access;
use memory_profiling::generate_flamegraph;

use crate::model::GitServerContext;

/// Generate a flamegraph of current memory allocations.
/// Requires the user to be a member of the SCM_PERF_ANALYSIS ACL group
/// (unless authorization is disabled via --skip-authorization flag).
pub async fn flamegraph(state: &mut State) -> Result<Response<Body>, HttpError> {
    // Extract values from state before async operations to avoid thread safety issues
    let (acl_provider, enforce_authorization) = {
        let git_ctx = GitServerContext::borrow_from(state);
        (git_ctx.acl_provider(), git_ctx.enforce_authorization())
    };

    // Check ACL access if authorization is enforced
    if enforce_authorization {
        let identities = {
            let request_ctx = match RequestContext::try_borrow_from(state) {
                Some(ctx) => ctx,
                None => {
                    return Err(HttpError::e400(anyhow!("Missing request context")));
                }
            };
            request_ctx.ctx.metadata().identities().clone()
        };

        check_acl_access(acl_provider.as_ref(), &identities).await?;
    }

    // Generate flamegraph
    let svg = generate_flamegraph().await?;

    let res = gotham::helpers::http::response::create_response(
        state,
        http::status::StatusCode::OK,
        "image/svg+xml".parse().unwrap(),
        svg,
    );
    Ok(res)
}
