// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use async_trait::async_trait;
use mononoke_api::{CoreContext, Mononoke};
use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use source_control::server::SourceControlService;
use source_control::services::source_control_service as service;
use source_control::types as thrift;
use sshrelay::SshEnvVars;
use tracing::TraceContext;
use uuid::Uuid;

trait ScubaInfoProvider {
    fn scuba_reponame(&self) -> Option<String> {
        None
    }
    fn scuba_commit(&self) -> Option<String> {
        None
    }
    fn scuba_path(&self) -> Option<String> {
        None
    }
}

#[derive(Clone)]
pub struct SourceControlServiceImpl {
    mononoke: Arc<Mononoke>,
    logger: Logger,
    scuba_builder: ScubaSampleBuilder,
}

impl SourceControlServiceImpl {
    pub fn new(mononoke: Arc<Mononoke>, logger: Logger, scuba_builder: ScubaSampleBuilder) -> Self {
        Self {
            mononoke,
            logger,
            scuba_builder,
        }
    }

    fn create_ctx(&self, scuba_info_provider: Option<&dyn ScubaInfoProvider>) -> CoreContext {
        let mut scuba = self.scuba_builder.clone();
        scuba.add_common_server_data().add("type", "thrift");
        if let Some(scuba_info_provider) = scuba_info_provider {
            if let Some(reponame) = scuba_info_provider.scuba_reponame() {
                scuba.add("reponame", reponame);
            }
            if let Some(commit) = scuba_info_provider.scuba_commit() {
                scuba.add("commit", commit);
            }
            if let Some(path) = scuba_info_provider.scuba_path() {
                scuba.add("path", path);
            }
        }
        let uuid = Uuid::new_v4();
        scuba.add("session_uuid", uuid.to_string());
        CoreContext::new(
            uuid,
            self.logger.clone(),
            scuba,
            None,
            TraceContext::default(),
            None,
            SshEnvVars::default(),
            None,
        )
    }
}

#[async_trait]
impl SourceControlService for SourceControlServiceImpl {
    async fn list_repos(
        &self,
        _params: thrift::ListReposParams,
    ) -> Result<Vec<thrift::Repo>, service::ListReposExn> {
        let _ctx = self.create_ctx(None);
        let rsp = self
            .mononoke
            .repo_names()
            .map(|repo_name| thrift::Repo {
                name: repo_name.to_string(),
            })
            .collect();
        Ok(rsp)
    }
}
