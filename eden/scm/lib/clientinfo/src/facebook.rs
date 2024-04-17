// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

use std::env::var;

use serde::Deserialize;
use serde::Serialize;

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
}

fn get_tw_job_handle() -> Option<String> {
    let job_cluster = var("TW_JOB_CLUSTER").ok()?;
    let job_user = var("TW_JOB_USER").ok()?;
    let job_name = var("TW_JOB_NAME").ok()?;

    Some(format!("{}/{}/{}", job_cluster, job_user, job_name))
}

pub fn get_fb_client_info() -> FbClientInfo {
    let tw_task = var("TW_TASK_ID").ok();

    FbClientInfo {
        tw_task,
        tw_job: get_tw_job_handle(),
        sandcastle_nonce: var("SANDCASTLE_NONCE").ok(),
        sandcastle_alias: var("SANDCASTLE_ALIAS").ok(),
        sandcastle_type: var("SANDCASTLE_TYPE").ok(),
    }
}
