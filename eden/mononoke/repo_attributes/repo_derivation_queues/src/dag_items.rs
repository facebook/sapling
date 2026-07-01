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
use derived_data_manager::DerivationStagePayload;
use derived_data_manager::StageKey;
use ephemeral_blobstore::BubbleId;
use fbthrift::compact_protocol;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use mononoke_types::MPathHash;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use serde::Deserialize;
use serde::Serialize;

/// Reserved suffix/serde token for the `StageKey::Finalize` variant. `finalize`
/// cannot collide with a hex-encoded `MPathHash`.
const FINALIZE_TOKEN: &str = "finalize";

/// Serde helper for `Option<StageKey>` — round-trips through a string so the
/// derived `Serialize`/`Deserialize` on `DagItemId` keeps working without
/// touching `StageKey` itself. The `Manifest` case is byte-identical to the
/// previous `Option<MPathHash>` encoding (hex string) so existing serialized
/// data is unaffected.
mod stage_id_serde {
    use std::str::FromStr;

    use derived_data_manager::StageKey;
    use mononoke_types::MPathHash;
    use serde::Deserialize;
    use serde::Deserializer;
    use serde::Serialize;
    use serde::Serializer;

    use super::FINALIZE_TOKEN;

    pub fn serialize<S: Serializer>(val: &Option<StageKey>, ser: S) -> Result<S::Ok, S::Error> {
        val.as_ref()
            .map(|v| match v {
                StageKey::Manifest(hash) => hash.to_hex().to_string(),
                StageKey::Finalize => FINALIZE_TOKEN.to_string(),
            })
            .serialize(ser)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Option<StageKey>, D::Error> {
        let s: Option<String> = Option::deserialize(de)?;
        s.map(|s| {
            if s == FINALIZE_TOKEN {
                Ok(StageKey::Finalize)
            } else {
                MPathHash::from_str(&s)
                    .map(StageKey::Manifest)
                    .map_err(serde::de::Error::custom)
            }
        })
        .transpose()
    }
}

use crate::InternalError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivationDagItem {
    pub dag_item_id: DagItemId,
    pub dag_item_info: DagItemInfo,
    pub deps: Vec<DagItemDep>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DagItemDep {
    pub dag_item_id: DagItemId,
    pub head_cs_id: ChangesetId,
    /// Absolute path of the dep's stage, for pipeline items. Carried alongside
    /// the dep so consumers (e.g. the queue's `check_derived`) don't need to
    /// translate `dag_item_id.stage_id` (a hash) back through the live config
    /// to recover the path. `None` for non-pipeline deps.
    #[serde(default)]
    pub stage_path: Option<MPath>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DagItemId {
    pub repo_id: RepositoryId,
    pub config_name: String,
    pub derived_data_type: DerivableType,
    pub root_cs_id: ChangesetId,
    /// Identity of the stage (hash of its absolute path), or `None` for
    /// non-pipeline items. Carried as a hash (not a name) so the queue identity
    /// survives stage renames in the live config — only the path matters.
    #[serde(default, with = "stage_id_serde")]
    pub stage_id: Option<StageKey>,
}

impl DagItemId {
    pub fn new(
        repo_id: RepositoryId,
        config_name: String,
        derived_data_type: DerivableType,
        root_cs_id: ChangesetId,
        stage_id: Option<StageKey>,
    ) -> Self {
        Self {
            repo_id,
            config_name,
            derived_data_type,
            root_cs_id,
            stage_id,
        }
    }

    pub fn suffix(&self) -> String {
        match &self.stage_id {
            Some(StageKey::Manifest(stage_hash)) => format!(
                "{}_{}:{}",
                self.derived_data_type,
                self.root_cs_id,
                stage_hash.to_hex(),
            ),
            Some(StageKey::Finalize) => format!(
                "{}_{}:{}",
                self.derived_data_type, self.root_cs_id, FINALIZE_TOKEN,
            ),
            None => format!("{}_{}", self.derived_data_type, self.root_cs_id),
        }
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
        // Suffix format is either:
        //   <derived_data_type>_<cs_id>                  (no stage)
        //   <derived_data_type>_<cs_id>:<stage_path_hex> (manifest pipeline item)
        //   <derived_data_type>_<cs_id>:finalize         (finalize pipeline item)
        let (base_suffix, stage_id) = match suffix.split_once(':') {
            Some((base, FINALIZE_TOKEN)) => (base, Some(StageKey::Finalize)),
            Some((base, stage_hex)) => (
                base,
                Some(StageKey::Manifest(
                    MPathHash::from_str(stage_hex).with_context(|| {
                        format!("While parsing stage hash from suffix {suffix}")
                    })?,
                )),
            ),
            None => (suffix, None),
        };

        let (data_type_str, cs_id_str) = base_suffix
            .rsplit_once('_')
            .ok_or_else(|| anyhow!("Invalid DagItemId suffix format: {suffix}"))?;

        let derived_data_type = data_type_str
            .parse::<DerivableType>()
            .with_context(|| format!("While parsing DerivableType from suffix {suffix}"))?;
        let root_cs_id = ChangesetId::from_str(cs_id_str)
            .with_context(|| format!("While parsing ChangesetId from suffix {suffix}"))?;

        Ok(Self {
            repo_id,
            config_name,
            derived_data_type,
            root_cs_id,
            stage_id,
        })
    }
}

fn default_derivation_priority() -> derivation_queue_thrift::DerivationPriority {
    derivation_queue_thrift::DerivationPriority::LOW
}

/// Convert a DerivationPriority to a string for logging.
pub fn derivation_priority_to_str(
    priority: derivation_queue_thrift::DerivationPriority,
) -> &'static str {
    match priority {
        derivation_queue_thrift::DerivationPriority::LOW => "low",
        derivation_queue_thrift::DerivationPriority::HIGH => "high",
        _ => "unknown",
    }
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
    #[serde(default)]
    stage_payload: Option<DerivationStagePayload>,
}

impl DagItemInfo {
    fn new(
        head_cs_id: ChangesetId,
        bubble_id: Option<BubbleId>,
        client_info: Option<&ClientInfo>,
        priority: derivation_queue_thrift::DerivationPriority,
        stage_payload: Option<DerivationStagePayload>,
    ) -> Self {
        let enqueue_timestamp = Some(Timestamp::now());
        Self {
            head_cs_id,
            bubble_id,
            enqueue_timestamp,
            client_info: client_info.cloned(),
            retry_count: 0,
            priority,
            stage_payload,
        }
    }

    pub fn priority(&self) -> derivation_queue_thrift::DerivationPriority {
        self.priority
    }

    pub fn stage_payload(&self) -> Option<&DerivationStagePayload> {
        self.stage_payload.as_ref()
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
            stage_payload: self.stage_payload.as_ref().map(|p| p.to_thrift()),
        }
    }

    fn from_thrift(dag_item_info: derivation_queue_thrift::DagItemInfo) -> Result<Self> {
        Ok(Self {
            head_cs_id: ChangesetId::from_thrift(dag_item_info.head_cs_id)?,
            bubble_id: dag_item_info
                .bubble_id
                .map(|bubble_id| {
                    BubbleId::try_from(bubble_id)
                        .map_err(|_| anyhow!("Invalid bubble id {bubble_id}"))
                })
                .transpose()?,
            enqueue_timestamp: dag_item_info.enqueue_timestamp.map(Timestamp::from_thrift),
            client_info: dag_item_info
                .client_info
                .as_deref()
                .and_then(|info| ClientInfo::from_json(info).ok()),
            retry_count: dag_item_info.retry_count.unwrap_or(0) as u64,
            priority: dag_item_info.priority,
            stage_payload: dag_item_info
                .stage_payload
                .map(DerivationStagePayload::from_thrift)
                .transpose()?,
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
        deps: Vec<DagItemDep>,
        client_info: Option<&ClientInfo>,
        priority: derivation_queue_thrift::DerivationPriority,
        stage_id: Option<StageKey>,
        stage_payload: Option<DerivationStagePayload>,
    ) -> Result<DerivationDagItem, InternalError> {
        debug_assert_eq!(
            stage_id.is_some(),
            stage_payload.is_some(),
            "stage_id and stage_payload must travel together: both Some (pipeline item) or both None (non-pipeline item)",
        );
        let dag_item_id = DagItemId {
            repo_id,
            config_name,
            derived_data_type,
            root_cs_id,
            stage_id,
        };
        let dag_item_info =
            DagItemInfo::new(head_cs_id, bubble_id, client_info, priority, stage_payload);
        if deps.iter().any(|d| d.dag_item_id == dag_item_id) {
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

    pub fn stage_id(&self) -> Option<&StageKey> {
        self.dag_item_id.stage_id.as_ref()
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

    pub fn priority(&self) -> derivation_queue_thrift::DerivationPriority {
        self.dag_item_info.priority
    }

    pub fn stage_payload(&self) -> Option<&DerivationStagePayload> {
        self.dag_item_info.stage_payload.as_ref()
    }

    /// The coupled `(stage_hash, payload)` pair for a pipeline item, or
    /// `None` for a non-pipeline item. Returns `None` if exactly one of the
    /// two fields is `Some` — that's a producer bug. Pairing the two into a
    /// single `Option` at the boundary keeps consumers from forgetting that
    /// they must always travel together.
    pub fn pipeline_stage(&self) -> Option<(&StageKey, &DerivationStagePayload)> {
        self.dag_item_id.stage_id.as_ref().zip(self.stage_payload())
    }

    pub fn deps(&self) -> &Vec<DagItemDep> {
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
            _ => Err(anyhow!("Couldn't parse {path} into DagItemId")),
        }
    }
}
