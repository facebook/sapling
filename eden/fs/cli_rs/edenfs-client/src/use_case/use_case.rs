/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// This value was selected semi-randomly and should be revisited in the future. Anecdotally, we
// have seen EdenFS struggle with <<< 2048 outstanding requests, but the exact number depends
// on the size/complexity/cost of the outstanding requests.
const DEFAULT_MAX_OUTSTANDING_REQUESTS: usize = 2048;

use std::path::Path;

use remote_loader::get_remote_configs;
use strum::IntoStaticStr;

use crate::use_case::helpers;
use crate::use_case::remote_config_snapshot::REMOTE_CONFIG_SNAPSHOT;
use crate::use_case::thrift_types::scm_usecases_types::ScmUseCases;

const CONFIG_HASH_INDEX: &str = "scm_usecases_current_config_hash";
const CONFIG_UP_TO_DATE_INDEX: &str = "scm_usecases_uptodate";
const CONFIG_INDEX: &str = "scm_usecases_config";

#[derive(Clone, Copy, Debug, Eq, IntoStaticStr, Hash, PartialEq)]
#[strum(serialize_all = "kebab_case")]
#[repr(u32)]
pub enum UseCaseId {
    #[strum(serialize = "buck2")]
    Buck2,
    #[strum(serialize = "edenfsctl")]
    EdenFsCtl,
    EdenFsTests,
    ExampleUseCase,
    Flow,
    Hack,
    MeerakatCli,
    RedirectFfi,
    #[strum(serialize = "testifyd")]
    TestifyDaemon,
    WatchActiveCommit,
}

pub struct UseCase {
    id: UseCaseId,
    oncall: String,
    max_outstanding_requests: usize,
}

impl UseCase {
    pub fn new(config_dir: &Path, id: UseCaseId) -> Self {
        let is_pub = cpe::x2p::supports_vpnless();
        let config_url = helpers::config_url(is_pub);
        //TODO: get proxy_sock_path (None for !is_pub, Some(http_proxy_path) for is_pub)
        let http_config =
            helpers::get_http_config(is_pub, None).expect("Failed to get http config");
        let cache_path = config_dir.join("scm_use_cases");
        let _scm_use_cases: ScmUseCases = get_remote_configs(
            is_pub,
            Some(config_url),
            300, // 5 minutes
            http_config,
            true, // allow_remote_snapshot
            &cache_path,
            REMOTE_CONFIG_SNAPSHOT,
            CONFIG_HASH_INDEX,
            CONFIG_UP_TO_DATE_INDEX,
            CONFIG_INDEX,
        )
        .expect("Failed to get remote configs");

        // TODO: use scm_use_cases...
        let oncall = match id {
            UseCaseId::Buck2 => "build_infra",
            UseCaseId::Flow => "flow",
            UseCaseId::Hack => "hack",
            _ => "scm_client_infra",
        };
        let max_outstanding_requests = DEFAULT_MAX_OUTSTANDING_REQUESTS;
        Self {
            id,
            oncall: oncall.to_string(),
            max_outstanding_requests,
        }
    }

    pub fn id(&self) -> &UseCaseId {
        &self.id
    }

    pub fn name(&self) -> &'static str {
        self.id.into()
    }

    pub fn oncall(&self) -> &str {
        &self.oncall
    }

    pub fn max_outstanding_requests(&self) -> usize {
        self.max_outstanding_requests
    }
}
