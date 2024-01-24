/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;

use clientinfo::ClientRequestInfo;
use tokio::task_local;

task_local! {
    pub static CLIENT_REQUEST_INFO_TASK_LOCAL: Option<ClientRequestInfo>;
}

pub fn get_client_request_info_task_local() -> Option<ClientRequestInfo> {
    CLIENT_REQUEST_INFO_TASK_LOCAL
        .try_with(Clone::clone)
        .ok()
        .flatten()
}

pub async fn with_client_request_info_scope<F: Future>(
    value: Option<ClientRequestInfo>,
    f: F,
) -> F::Output {
    CLIENT_REQUEST_INFO_TASK_LOCAL.scope(value, f).await
}
