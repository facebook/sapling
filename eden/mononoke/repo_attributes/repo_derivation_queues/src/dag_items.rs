/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use bytes::Bytes;
use clientinfo::ClientInfo;
use derived_data_manager::DerivableType;
use ephemeral_blobstore::BubbleId;
use fbthrift::compact_protocol;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use serde::Deserialize;
use serde::Serialize;

use crate::InternalError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivationDagItem {
    pub dag_item_id: DagItemId,
    pub dag_item_info: DagItemInfo,
    pub deps: Vec<DagItemId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DagItemId {
    pub repo_id: RepositoryId,
    pub config_name: String,
    pub derived_data_type: DerivableType,
    pub root_cs_id: ChangesetId,
}

impl DagItemId {
    pub fn new(
        repo_id: RepositoryId,
        config_name: String,
        derived_data_type: DerivableType,
        root_cs_id: ChangesetId,
    ) -> Self {
        Self {
            repo_id,
            config_name,
            derived_data_type,
            root_cs_id,
        }
    }

    pub fn suffix(&self) -> String {
        format!("{}_{}", self.derived_data_type, self.root_cs_id)
    }

    pub fn config_name(&self) -> &str {
        &self.config_name
    }

    pub fn derived_data_type(&self) -> DerivableType {
        self.derived_data_type
    }

    pub fn root_cs_id(&self) -> ChangesetId {
        self.root_cs_id
    }

    pub fn from_suffix(suffix: &str, repo_id: RepositoryId, config_name: String) -> Result<Self> {
        let (data_type_str, cs_id_str) = suffix
            .rsplit_once('_')
            .ok_or_else(|| anyhow!("Invalid DagItemId suffix format: {}", suffix))?;

        let derived_data_type = data_type_str
            .parse::<DerivableType>()
            .with_context(|| format!("While parsing DerivableType from suffix {}", suffix))?;
        let root_cs_id = ChangesetId::from_str(cs_id_str)
            .with_context(|| format!("While parsing ChangesetId from suffix {}", suffix))?;

        Ok(Self {
            repo_id,
            config_name,
            derived_data_type,
            root_cs_id,
        })
    }
}

fn default_derivation_priority() -> derivation_queue_thrift::DerivationPriority {
    derivation_queue_thrift::DerivationPriority::LOW
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagItemInfo {
    head_cs_id: ChangesetId,
    bubble_id: Option<BubbleId>,
    enqueue_timestamp: Option<Timestamp>,
    client_info: Option<ClientInfo>,
    retry_count: u64,
    #[serde(default = "default_derivation_priority")]
    priority: derivation_queue_thrift::DerivationPriority,
}

impl DagItemInfo {
    fn new(
        head_cs_id: ChangesetId,
        bubble_id: Option<BubbleId>,
        client_info: Option<&ClientInfo>,
        priority: derivation_queue_thrift::DerivationPriority,
    ) -> Self {
        let enqueue_timestamp = Some(Timestamp::now());
        Self {
            head_cs_id,
            bubble_id,
            enqueue_timestamp,
            client_info: client_info.cloned(),
            retry_count: 0,
            priority,
        }
    }

    pub fn priority(&self) -> derivation_queue_thrift::DerivationPriority {
        self.priority
    }

    fn to_thrift(&self) -> derivation_queue_thrift::DagItemInfo {
        derivation_queue_thrift::DagItemInfo {
            head_cs_id: self.head_cs_id.into_thrift(),
            bubble_id: self.bubble_id.map(|bubble_id| bubble_id.into()),
            enqueue_timestamp: self.enqueue_timestamp.map(Timestamp::into_thrift),
            client_info: self
                .client_info
                .as_ref()
                .and_then(|info| info.to_json().ok()),
            retry_count: Some(self.retry_count as i64),
            priority: self.priority,
        }
    }

    fn from_thrift(dag_item_info: derivation_queue_thrift::DagItemInfo) -> Result<Self> {
        Ok(Self {
            head_cs_id: ChangesetId::from_thrift(dag_item_info.head_cs_id)?,
            bubble_id: dag_item_info
                .bubble_id
                .map(|bubble_id| {
                    BubbleId::try_from(bubble_id)
                        .map_err(|_| anyhow!("Invalid bubble id {}", bubble_id))
                })
                .transpose()?,
            enqueue_timestamp: dag_item_info.enqueue_timestamp.map(Timestamp::from_thrift),
            client_info: dag_item_info
                .client_info
                .as_deref()
                .and_then(|info| ClientInfo::from_json(info).ok()),
            retry_count: dag_item_info.retry_count.unwrap_or(0) as u64,
            priority: dag_item_info.priority,
        })
    }

    pub fn serialize(&self) -> Bytes {
        compact_protocol::serialize(self.to_thrift())
    }

    pub fn deserialize(data: &[u8]) -> Result<Self> {
        Self::from_thrift(compact_protocol::deserialize(data)?)
    }

    pub fn increment_retry_count(&mut self) {
        self.retry_count += 1;
    }

    pub fn retry_count(&self) -> u64 {
        self.retry_count
    }

    pub fn set_priority(&mut self, priority: derivation_queue_thrift::DerivationPriority) {
        self.priority = priority;
    }

    pub fn head_cs_id(&self) -> ChangesetId {
        self.head_cs_id
    }

    pub fn enqueue_timestamp(&self) -> Option<Timestamp> {
        self.enqueue_timestamp
    }

    pub fn client_info(&self) -> Option<&ClientInfo> {
        self.client_info.as_ref()
    }

    pub fn bubble_id(&self) -> Option<BubbleId> {
        self.bubble_id
    }
}

impl DerivationDagItem {
    pub fn new(
        repo_id: RepositoryId,
        config_name: String,
        derived_data_type: DerivableType,
        root_cs_id: ChangesetId,
        head_cs_id: ChangesetId,
        bubble_id: Option<BubbleId>,
        deps: Vec<DagItemId>,
        client_info: Option<&ClientInfo>,
        priority: derivation_queue_thrift::DerivationPriority,
    ) -> Result<DerivationDagItem, InternalError> {
        let dag_item_id = DagItemId {
            repo_id,
            config_name,
            derived_data_type,
            root_cs_id,
        };
        let dag_item_info = DagItemInfo::new(head_cs_id, bubble_id, client_info, priority);
        if deps.contains(&dag_item_id) {
            return Err(InternalError::CircularDependency(dag_item_id));
        }
        Ok(Self {
            dag_item_id,
            dag_item_info,
            deps,
        })
    }

    pub fn id(&self) -> &DagItemId {
        &self.dag_item_id
    }

    pub fn info(&self) -> &DagItemInfo {
        &self.dag_item_info
    }

    pub fn repo_id(&self) -> RepositoryId {
        self.dag_item_id.repo_id
    }

    pub fn config_name(&self) -> &str {
        &self.dag_item_id.config_name
    }

    pub fn derived_data_type(&self) -> DerivableType {
        self.dag_item_id.derived_data_type
    }

    pub fn root_cs_id(&self) -> ChangesetId {
        self.dag_item_id.root_cs_id
    }

    pub fn head_cs_id(&self) -> ChangesetId {
        self.dag_item_info.head_cs_id
    }

    pub fn bubble_id(&self) -> Option<BubbleId> {
        self.dag_item_info.bubble_id
    }

    pub fn enqueue_timestamp(&self) -> Option<Timestamp> {
        self.dag_item_info.enqueue_timestamp
    }

    pub fn client_info(&self) -> Option<&ClientInfo> {
        self.dag_item_info.client_info.as_ref()
    }

    pub fn retry_count(&self) -> u64 {
        self.dag_item_info.retry_count
    }

    pub fn deps(&self) -> &Vec<DagItemId> {
        &self.deps
    }
}

impl TryFrom<&str> for DagItemId {
    type Error = anyhow::Error;

    fn try_from(path: &str) -> Result<Self> {
        // expecting format `/mononoke_derivation/<node_type>/<repo_id>/<config_name>/<data_type>_<root_cs_id>`
        let items: Vec<&str> = path.split('/').collect();
        match items[..] {
            ["", _, _, repo_id_str, config_name, suffix] => {
                let repo_id = repo_id_str.parse::<RepositoryId>()?;
                Self::from_suffix(suffix, repo_id, config_name.to_string())
            }
            _ => Err(anyhow!("Couldn't parse {} into DagItemId", path)),
        }
    }
}
