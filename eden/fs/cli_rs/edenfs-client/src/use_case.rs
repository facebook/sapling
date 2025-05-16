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

use std::collections::HashSet;

use anyhow::Result;
use anyhow::bail;
use strum::IntoStaticStr;

#[derive(Clone, Copy, Debug, Eq, IntoStaticStr, Hash, PartialEq)]
#[strum(serialize_all = "kebab_case")]
#[repr(u32)]
pub enum UseCaseId {
    #[strum(serialize = "edenfsctl")]
    EdenFsCtl,
    EdenFsTests,
    ExampleUseCase,
    MeerakatCli,
    RedirectFfi,
    #[strum(serialize = "testifyd")]
    TestifyDaemon,
    WatchActiveCommit,
    Hack,
}

pub struct UseCase {
    id: UseCaseId,
    oncall: String,
    max_outstanding_requests: usize,
}

impl UseCase {
    pub fn new(id: UseCaseId) -> Self {
        // TODO: retrieve use case specifics from configerator
        let oncall = match id {
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

#[allow(dead_code)]
fn config_url(is_pub: bool) -> String {
    format!("https://{}/hg/config/", interngraph_host(is_pub))
}

#[allow(dead_code)]
fn interngraph_host(is_pub: bool) -> &'static str {
    if is_pub {
        "interngraph.internmc.facebook.com"
    } else {
        "interngraph.intern.facebook.com"
    }
}

#[allow(dead_code)]
fn get_http_config(is_pub: bool, proxy_sock_path: Option<&str>) -> Result<http_client::Config> {
    let mut http_config = http_client::Config::default();
    if is_pub {
        let proxy_sock = match proxy_sock_path {
            Some(path) => path.to_string(),
            None => bail!("no proxy_sock_path when fetching remote config in pub domain"),
        };

        let intern_host = interngraph_host(is_pub);

        http_config.unix_socket_path = Some(proxy_sock);
        http_config.unix_socket_domains = HashSet::from([intern_host.to_string()]);
    }
    Ok(http_config)
}
