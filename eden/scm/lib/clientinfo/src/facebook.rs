/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::env::var;

use atlas_whoami::AtlasWhoAmI;
use atlas_whoami::Purpose;
use cross_env_session_id::CrossEnvironmentSessionId;
use serde::Deserialize;
use serde::Serialize;

pub fn log_cross_environment_session_id() -> String {
    let cesi = CrossEnvironmentSessionId::get().unwrap_or(String::new());
    tracing::info!(target: "clienttelemetry", cross_environment_session_id=cesi);
    cesi
}

#[derive(Default, Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct FbClientInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    tw_job: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tw_task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sandcastle_nonce: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sandcastle_alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sandcastle_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sandcastle_vcs: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    atlas: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    atlas_rl: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    atlas_env_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    faas_job_name: Option<String>,
}

impl FbClientInfo {
    pub fn tw_job(&self) -> Option<&str> {
        self.tw_job.as_deref()
    }

    pub fn tw_task(&self) -> Option<&str> {
        self.tw_task.as_deref()
    }

    pub fn sandcastle_nonce(&self) -> Option<&str> {
        self.sandcastle_nonce.as_deref()
    }

    pub fn sandcastle_alias(&self) -> Option<&str> {
        self.sandcastle_alias.as_deref()
    }

    pub fn sandcastle_type(&self) -> Option<&str> {
        self.sandcastle_type.as_deref()
    }

    pub fn sandcastle_vcs(&self) -> Option<&str> {
        self.sandcastle_vcs.as_deref()
    }

    pub fn is_atlas(&self) -> Option<bool> {
        self.atlas
    }

    pub fn is_atlas_rl(&self) -> Option<bool> {
        self.atlas_rl
    }

    pub fn atlas_env_id(&self) -> Option<&str> {
        self.atlas_env_id.as_deref()
    }

    pub fn faas_job_name(&self) -> Option<&str> {
        self.faas_job_name.as_deref()
    }
}

/// Detect an Atlas-style boolean env var the same way as the config loader's
/// `platform_helpers::is_atlas` (set to "1" means true). Returns `None` when
/// unset so the field stays absent for non-Atlas clients. We can't reuse that
/// helper directly: it lives in `configloader`, and clientinfo -> configloader
/// would be a dependency cycle (configloader -> http-client -> clientinfo).
fn atlas_env_flag(name: &str) -> Option<bool> {
    std::env::var_os(name).map(|v| v == "1")
}

/// Whether this is a reinforcement-learning Atlas container, read from the
/// `/etc/atlaswhoami` identity file written by the Atlas preparer. `None` when
/// there is no whoami file (non-Atlas client) or no purpose recorded, mirroring
/// the absent-field behaviour of the other optional fields.
fn atlas_rl_from_whoami() -> Option<bool> {
    let purpose = AtlasWhoAmI::get().ok()?.purpose?;
    Some(purpose == Purpose::ReinforcementLearning)
}

fn get_tw_job_handle() -> Option<String> {
    let job_cluster = var("TW_JOB_CLUSTER").ok()?;
    let job_user = var("TW_JOB_USER").ok()?;
    let job_name = var("TW_JOB_NAME").ok()?;

    Some(format!("{job_cluster}/{job_user}/{job_name}"))
}

pub fn get_fb_client_info() -> FbClientInfo {
    let tw_task = var("TW_TASK_ID").ok();

    FbClientInfo {
        tw_task,
        tw_job: get_tw_job_handle(),
        sandcastle_nonce: var("SANDCASTLE_NONCE").ok(),
        sandcastle_alias: var("SANDCASTLE_ALIAS").ok(),
        sandcastle_type: var("SANDCASTLE_TYPE").ok(),
        sandcastle_vcs: var("SANDCASTLE_VCS").ok(),
        atlas: atlas_env_flag("ATLAS"),
        atlas_rl: atlas_rl_from_whoami(),
        atlas_env_id: var("ATLAS_ENV_ID").ok(),
        faas_job_name: var("FAAS_JOB_NAME").ok(),
    }
}
