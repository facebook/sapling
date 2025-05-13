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
}

pub struct UseCase {
    id: UseCaseId,
    oncall: String,
    max_outstanding_requests: usize,
}

impl UseCase {
    pub fn new(id: UseCaseId) -> Self {
        // TODO: retrieve use case specifics from configerator
        let oncall = "scm_client_infra".to_string();
        let max_outstanding_requests = DEFAULT_MAX_OUTSTANDING_REQUESTS;
        Self {
            id,
            oncall,
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
