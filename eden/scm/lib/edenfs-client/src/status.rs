/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_runtime::block_on;
use clientinfo::get_client_request_info;
use eden::GetScmStatusParams;
use eden::GetScmStatusResult;
use thrift_types::edenfs as eden;
use thrift_types::edenfs::RootIdOptions;
use types::HgId;

use crate::client::EdenFsClient;

pub fn get_status(
    commit: HgId,
    ignored: bool,
    client: &EdenFsClient,
) -> Result<GetScmStatusResult> {
    block_on(get_status_internal(commit, ignored, client))
}

async fn get_status_internal(
    commit: HgId,
    ignored: bool,
    client: &EdenFsClient,
) -> Result<GetScmStatusResult> {
    let thrift_client = client.get_thrift_client().await?;
    let slcri = get_client_request_info();
    let cri = eden::ClientRequestInfo {
        correlator: slcri.correlator,
        entry_point: slcri.entry_point.to_string(),
        ..Default::default()
    };
    let filter_id = client.get_active_filter_id(commit.clone())?;
    let root_id_options = RootIdOptions {
        filterId: filter_id,
        ..Default::default()
    };
    thrift_client
        .getScmStatusV2(&GetScmStatusParams {
            mountPoint: client.root().as_bytes().to_vec(),
            commit: commit.into_byte_array().into(),
            listIgnored: ignored,
            cri: Some(cri),
            rootIdOptions: Some(root_id_options),
            ..Default::default()
        })
        .await
        .map_err(|err| err.into())
}
