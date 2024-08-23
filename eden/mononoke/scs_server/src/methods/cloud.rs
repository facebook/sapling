/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;

use crate::errors;
use crate::methods::thrift;
use crate::source_control_impl::SourceControlServiceImpl;

impl SourceControlServiceImpl {
    pub async fn cloud_workspace_info(
        &self,
        _ctx: CoreContext,
        _params: thrift::CloudWorkspaceInfoParams,
    ) -> Result<thrift::CloudWorkspaceInfoResponse, errors::ServiceError> {
        unimplemented!("cloud_workspace_info is not implemented yet on scs")
    }
}
