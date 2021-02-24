/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;
use source_control as thrift;

use crate::errors;
use crate::source_control_impl::SourceControlServiceImpl;

impl SourceControlServiceImpl {
    pub(crate) async fn megarepo_add_sync_target_config(
        &self,
        _ctx: CoreContext,
        _params: thrift::MegarepoAddConfigParams,
    ) -> Result<thrift::MegarepoAddConfigResponse, errors::ServiceError> {
        Err(errors::ServiceError::from(errors::not_implemented(
            "megarepo_add_sync_target_config is not yet implemented",
        )))
    }

    pub(crate) async fn megarepo_add_sync_target(
        &self,
        _ctx: CoreContext,
        _params: thrift::MegarepoAddTargetParams,
    ) -> Result<thrift::MegarepoAddTargetResponse, errors::ServiceError> {
        Err(errors::ServiceError::from(errors::not_implemented(
            "megarepo_add_sync_target is not yet implemented",
        )))
    }

    pub(crate) async fn megarepo_change_target_config(
        &self,
        _ctx: CoreContext,
        _params: thrift::MegarepoChangeTargetConfigParams,
    ) -> Result<thrift::MegarepoChangeConfigToken, errors::ServiceError> {
        Err(errors::ServiceError::from(errors::not_implemented(
            "megarepo_change_target_config is not yet implemented",
        )))
    }

    pub(crate) async fn megarepo_change_target_config_poll(
        &self,
        _ctx: CoreContext,
        _params: thrift::MegarepoChangeConfigToken,
    ) -> Result<thrift::MegarepoChangeTargetConfigResponse, errors::ServiceError> {
        Err(errors::ServiceError::from(errors::not_implemented(
            "poll_megarepo_change_config is not yet implemented",
        )))
    }

    pub(crate) async fn megarepo_sync_changeset(
        &self,
        _ctx: CoreContext,
        _params: thrift::MegarepoSyncChangesetParams,
    ) -> Result<thrift::MegarepoSyncChangesetToken, errors::ServiceError> {
        Err(errors::ServiceError::from(errors::not_implemented(
            "megarepo_sync_changeset is not yet implemented",
        )))
    }

    pub(crate) async fn megarepo_sync_changeset_poll(
        &self,
        _ctx: CoreContext,
        _params: thrift::MegarepoSyncChangesetToken,
    ) -> Result<thrift::MegarepoSyncChangesetResponse, errors::ServiceError> {
        Err(errors::ServiceError::from(errors::not_implemented(
            "poll_megarepo_sync_changeset is not yet implemented",
        )))
    }

    pub(crate) async fn megarepo_remerge_source(
        &self,
        _ctx: CoreContext,
        _params: thrift::MegarepoRemergeSourceParams,
    ) -> Result<thrift::MegarepoRemergeSourceToken, errors::ServiceError> {
        Err(errors::ServiceError::from(errors::not_implemented(
            "megarepo_remerge_source is not yet implemented",
        )))
    }

    pub(crate) async fn megarepo_remerge_source_poll(
        &self,
        _ctx: CoreContext,
        _params: thrift::MegarepoRemergeSourceToken,
    ) -> Result<thrift::MegarepoRemergeSourceResponse, errors::ServiceError> {
        Err(errors::ServiceError::from(errors::not_implemented(
            "poll_megarepo_remerge_source is not yet implemented",
        )))
    }
}
