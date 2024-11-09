/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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
