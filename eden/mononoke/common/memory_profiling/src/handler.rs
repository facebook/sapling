/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use gotham_ext::error::HttpError;
use http::StatusCode;
use permission_checker::AclProvider;
use permission_checker::MononokeIdentitySet;

/// Check if jemalloc profiling is available.
///
/// Returns Ok(()) if profiling is available, or an HttpError with status 422
/// if the server was not started with jemalloc profiling enabled.
#[cfg(fbcode_build)]
pub fn check_profiling_available() -> Result<(), HttpError> {
    let _prof_ctl = jemalloc_pprof::PROF_CTL.as_ref().ok_or_else(|| HttpError {
        error: anyhow!(
            "Memory profiling not available. Server must be started with MALLOC_CONF=prof:true,prof_active:true"
        ),
        status_code: StatusCode::UNPROCESSABLE_ENTITY,
    })?;

    Ok(())
}

#[cfg(not(fbcode_build))]
pub fn check_profiling_available() -> Result<(), HttpError> {
    Err(HttpError {
        error: anyhow!("Memory profiling not available in OSS builds"),
        status_code: StatusCode::UNPROCESSABLE_ENTITY,
    })
}

/// Check if user is in SCM_PERF_ANALYSIS group.
///
/// Returns Ok(()) if the user is a member of the SCM_PERF_ANALYSIS group,
/// or an HttpError with status 403 if access is denied or cannot be verified.
pub async fn check_acl_access(
    acl_provider: &dyn AclProvider,
    identities: &MononokeIdentitySet,
) -> Result<(), HttpError> {
    let group = acl_provider
        .group("SCM_PERF_ANALYSIS")
        .await
        .map_err(|e| HttpError::e403(e.context("Failed to verify SCM_PERF_ANALYSIS membership")))?;

    let is_member = group.is_member(identities).await;

    if !is_member {
        return Err(HttpError::e403(anyhow!(
            "Access denied: user is not a member of SCM_PERF_ANALYSIS"
        )));
    }

    Ok(())
}

/// Generate flamegraph SVG and return as bytes.
///
/// Returns the SVG content as a Vec<u8> on success, or an HttpError if:
/// - Jemalloc profiling is not available (422)
/// - Profiling is not activated (422)
/// - Flamegraph generation fails (422)
#[cfg(fbcode_build)]
pub async fn generate_flamegraph() -> Result<Vec<u8>, HttpError> {
    let prof_ctl = jemalloc_pprof::PROF_CTL
        .as_ref()
        .ok_or_else(|| HttpError {
            error: anyhow!(
                "Memory profiling not available. Server must be started with MALLOC_CONF=prof:true,prof_active:true"
            ),
            status_code: StatusCode::UNPROCESSABLE_ENTITY,
        })?;

    let mut guard = prof_ctl.lock().await;

    if !guard.activated() {
        return Err(HttpError {
            error: anyhow!(
                "Memory profiling not activated. Server must be started with MALLOC_CONF=prof:true,prof_active:true"
            ),
            status_code: StatusCode::UNPROCESSABLE_ENTITY,
        });
    }

    let svg = guard.dump_flamegraph().map_err(|e| HttpError {
        error: anyhow!("Failed to generate flamegraph: {}", e),
        status_code: StatusCode::UNPROCESSABLE_ENTITY,
    })?;

    Ok(svg)
}

#[cfg(not(fbcode_build))]
pub async fn generate_flamegraph() -> Result<Vec<u8>, HttpError> {
    Err(HttpError {
        error: anyhow!("Memory profiling not available in OSS builds"),
        status_code: StatusCode::UNPROCESSABLE_ENTITY,
    })
}
