/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use bytes::Bytes;
use clientinfo::ClientInfo;
use derived_data_manager::DerivableType;
use fbthrift::compact_protocol;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;

use crate::InternalError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivationDagItem {
    pub dag_item_id: DagItemId,
    pub dag_item_info: DagItemInfo,
    pub deps: Vec<DagItemId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DagItemInfo {
    head_cs_id: ChangesetId,
    enqueue_timestamp: Option<Timestamp>,
    client_info: Option<ClientInfo>,
}

impl DagItemInfo {
    fn new(head_cs_id: ChangesetId, client_info: Option<&ClientInfo>) -> Self {
        let enqueue_timestamp = Some(Timestamp::now());
        Self {
            head_cs_id,
            enqueue_timestamp,
            client_info: client_info.cloned(),
        }
    }

    fn to_thrift(&self) -> derivation_queue_thrift::DagItemInfo {
        derivation_queue_thrift::DagItemInfo {
            head_cs_id: self.head_cs_id.into_thrift(),
            enqueue_timestamp: self.enqueue_timestamp.map(Timestamp::into_thrift),
            client_info: self
                .client_info
                .as_ref()
                .and_then(|info| info.to_json().ok()),
        }
    }

    fn from_thrift(dag_item_info: derivation_queue_thrift::DagItemInfo) -> Result<Self> {
        Ok(Self {
            head_cs_id: ChangesetId::from_thrift(dag_item_info.head_cs_id)?,
            enqueue_timestamp: dag_item_info.enqueue_timestamp.map(Timestamp::from_thrift),
            client_info: dag_item_info
                .client_info
                .as_deref()
                .and_then(|info| ClientInfo::from_json(info).ok()),
        })
    }

    pub fn serialize(&self) -> Bytes {
        compact_protocol::serialize(self.to_thrift())
    }

    pub fn deserialize(data: &[u8]) -> Result<Self> {
        if data.len() == 32 {
            // Old format: data is just the head changset id.
            let head_cs_id = ChangesetId::from_bytes(data)?;
            Ok(Self {
                head_cs_id,
                enqueue_timestamp: None,
                client_info: None,
            })
        } else {
            // New format: deserialize thrift.
            Self::from_thrift(compact_protocol::deserialize(data)?)
        }
    }
}

impl DerivationDagItem {
    pub fn new(
        repo_id: RepositoryId,
        config_name: String,
        derived_data_type: DerivableType,
        root_cs_id: ChangesetId,
        head_cs_id: ChangesetId,
        deps: Vec<DagItemId>,
        client_info: Option<&ClientInfo>,
    ) -> Result<DerivationDagItem, InternalError> {
        let dag_item_id = DagItemId {
            repo_id,
            config_name,
            derived_data_type,
            root_cs_id,
        };
        let dag_item_info = DagItemInfo::new(head_cs_id, client_info);
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

    pub fn enqueue_timestamp(&self) -> Option<Timestamp> {
        self.dag_item_info.enqueue_timestamp
    }

    pub fn client_info(&self) -> Option<&ClientInfo> {
        self.dag_item_info.client_info.as_ref()
    }

    pub fn deps(&self) -> &Vec<DagItemId> {
        &self.deps
    }
}

impl TryFrom<&str> for DagItemId {
    type Error = anyhow::Error;

    fn try_from(path: &str) -> Result<Self> {
        // expecting format `/mononoke_derivation/<node_type>/<repo_id>/<config_name>/<data_type>_<root_cs_id>`
        // skip leading '/' and split the prefix into parts 5
        let items: Vec<&str> = path.split('/').collect();
        match items[..] {
            ["", _, _, a, b, c] => {
                let repo_id = a.parse::<RepositoryId>()?;
                let config_name = b.to_string();
                // parse part like `Unodes_30e4c306c0d74cdf898b23df9c61fd14eec6df7013964e537f53343efe7b30c3'
                let (derived_data_type, root_cs_id) = match c.rsplit_once('_') {
                    Some((data, cs)) => (
                        data.parse::<DerivableType>().with_context(|| {
                            format!("While parsing Derived Data Type from {}", b)
                        })?,
                        ChangesetId::from_str(cs)?,
                    ),
                    None => {
                        return Err(anyhow!("Couldn't parse DerivationDagItem from {}", path));
                    }
                };
                Ok(Self {
                    repo_id,
                    config_name,
                    derived_data_type,
                    root_cs_id,
                })
            }
            _ => Err(anyhow!("Couldn't parse {} into DerivationDagItem", path)),
        }
    }
}
