/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::PathBuf;

use remote_loader::get_remote_configs;
use strum::EnumString;
use strum::IntoStaticStr;

use crate::use_case::helpers;
use crate::use_case::remote_config_snapshot::REMOTE_CONFIG_SNAPSHOT;
use crate::use_case::thrift_types::scm_usecases_types::ScmUseCase;
use crate::use_case::thrift_types::scm_usecases_types::ScmUseCases;

// Used for fallback cases only.
const DEFAULT_ON_CALL: &str = "unknown";
const DEFAULT_MAX_CONCURRENT_REQUESTS: usize = 32;

const CONFIG_HASH_INDEX: &str = "scm_usecases_current_config_hash";
const CONFIG_UP_TO_DATE_INDEX: &str = "scm_usecases_uptodate";
const CONFIG_INDEX: &str = "scm_usecases_config";

#[derive(Clone, Copy, Debug, Eq, EnumString, IntoStaticStr, Hash, PartialEq)]
#[strum(serialize_all = "kebab_case")]
#[repr(u32)]
pub enum UseCaseId {
    #[strum(serialize = "buck2")]
    Buck2,
    Debugging,
    #[strum(serialize = "edenfsctl")]
    EdenFsCtl,
    EdenFsTests,
    ExampleUseCase,
    Flow,
    Hack,
    MeerkatCli,
    ScmEdenContainer,
    RedirectFfi,
    #[strum(serialize = "testifyd")]
    TestifyDaemon,
    WatchActiveCommit,
    StarlarkMcp,
}

pub struct UseCase {
    config_dir: PathBuf,
    id: UseCaseId,
}

impl UseCase {
    pub fn new(config_dir: &Path, id: UseCaseId) -> Self {
        Self {
            config_dir: config_dir.to_path_buf(),
            id,
        }
    }

    pub fn id(&self) -> &UseCaseId {
        &self.id
    }

    pub fn name(&self) -> &'static str {
        self.id.into()
    }

    pub fn oncall(&self) -> String {
        self.get_use_case()
            .map_or(DEFAULT_ON_CALL.to_string(), |use_case| {
                use_case.oncall.clone()
            })
    }

    pub fn max_concurrent_requests(&self) -> usize {
        self.get_use_case()
            .map_or(DEFAULT_MAX_CONCURRENT_REQUESTS, |use_case| {
                // A 0 value is prevent in configerator, so we can assume that
                // try_into will never return 0.
                use_case
                    .config
                    .edenfs_limits
                    .max_concurrent_requests
                    .try_into()
                    .unwrap_or(DEFAULT_MAX_CONCURRENT_REQUESTS)
            })
    }

    fn get_use_case(&self) -> Option<ScmUseCase> {
        let is_pub = cpe::x2p::supports_vpnless();
        let config_url = helpers::config_url(is_pub);
        let proxy_url = cpe::x2p::proxy_url_http1();
        let proxy_sock_path: Option<&str> = if proxy_url.is_empty() {
            None
        } else {
            Some(&proxy_url)
        };
        let http_config = helpers::get_http_config(is_pub, proxy_sock_path).ok()?;
        let cache_path = self.config_dir.join("scm_use_cases");
        let config: ScmUseCases = get_remote_configs(
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
        .ok()?;
        config.use_cases.get(self.name()).cloned()
    }
}
