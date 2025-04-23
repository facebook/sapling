/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use blobstore::Blobstore;
use context::CoreContext;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use manifest::ManifestOps;
use manifest::ManifestParentReplacement;
use manifest::StoreLoadable;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types::subtree_change::SubtreeChange;
use mononoke_types::subtree_change::SubtreeCopy;
use mononoke_types::subtree_change::SubtreeDeepCopy;
use mononoke_types::subtree_change::SubtreeImport;
use mononoke_types::subtree_change::SubtreeMerge;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use sorted_vector_map::SortedVectorMap;

use crate::HgChangesetId;
use crate::HgFileNodeId;
use crate::HgManifestId;

/// Mercurial-encoded subtree changes.  This contains subtree changes as defined
/// in the `subtree` extra field of a commit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HgSubtreeChanges {
    pub copies: Vec<HgSubtreeCopy>,
    pub deep_copies: Vec<HgSubtreeDeepCopy>,
    pub merges: Vec<HgSubtreeMerge>,
    pub imports: Vec<HgSubtreeImport>,
}

impl HgSubtreeChanges {
    /// Convert from JSON (as defined in sapling/utils/subtreeutils.py).
    pub fn from_json(json: &[u8]) -> Result<Self> {
        let all_changes: Vec<HgSubtreeChangesVersion> =
            serde_json::from_slice(json).context("Failed to parse subtree branch info")?;
        let mut copies = Vec::new();
        let mut deep_copies = Vec::new();
        let mut merges = Vec::new();
        let mut imports = Vec::new();
        for changes in all_changes {
            copies.extend(changes.copies);
            deep_copies.extend(changes.deepcopies);
            merges.extend(changes.merges);
            imports.extend(changes.imports);
        }
        Ok(HgSubtreeChanges {
            copies,
            deep_copies,
            merges,
            imports,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.copies.is_empty()
            && self.deep_copies.is_empty()
            && self.merges.is_empty()
            && self.imports.is_empty()
    }

    /// Get the set of changeset ids that are referenced by this set of changes.
    /// This does not include ids from external repositories that have been imported.
    pub fn source_changeset_ids(&self) -> HashSet<HgChangesetId> {
        let mut ids = HashSet::new();
        ids.extend(self.copies.iter().map(|copy| copy.from_commit));
        ids.extend(self.deep_copies.iter().map(|copy| copy.from_commit));
        ids.extend(self.merges.iter().map(|merge| merge.from_commit));
        ids
    }

    /// Convert to JSON (as defined in sapling/utils/subtreeutils.py).
    pub fn to_json(&self) -> Result<String> {
        if self.is_empty() {
            return Ok("[]".to_string());
        }
        // Currently we only support version 1.
        let all_changes = vec![HgSubtreeChangesVersion {
            copies: self.copies.clone(),
            deepcopies: self.deep_copies.clone(),
            merges: self.merges.clone(),
            imports: self.imports.clone(),
            version: 1,
        }];
        let json = serde_json::to_string(&all_changes).context("Failed to serialize changes")?;
        Ok(json)
    }

    pub async fn to_manifest_replacements(
        &self,
        ctx: &CoreContext,
        blobstore: &Arc<dyn Blobstore>,
    ) -> Result<Vec<ManifestParentReplacement<HgManifestId, (FileType, HgFileNodeId)>>> {
        // Deep copies, merges and imports do not modify the parents: they just adjust history.
        stream::iter(self.copies.iter())
            .map(|copy| Ok(async move { copy.to_manifest_replacement(ctx, blobstore).await }))
            .try_buffered(10)
            .try_collect::<Vec<_>>()
            .boxed()
            .await
    }

    pub fn from_bonsai_subtree_changes(
        subtree_changes: &SortedVectorMap<MPath, SubtreeChange>,
        subtree_change_sources: HashMap<ChangesetId, HgChangesetId>,
    ) -> Result<Option<Self>> {
        if subtree_changes.is_empty() {
            return Ok(None);
        }
        let mut copies = Vec::new();
        let mut deep_copies = Vec::new();
        let mut merges = Vec::new();
        let mut imports = Vec::new();
        for (path, change) in subtree_changes.iter() {
            match change {
                SubtreeChange::SubtreeCopy(SubtreeCopy {
                    from_path,
                    from_cs_id,
                }) => {
                    copies.push(HgSubtreeCopy {
                        from_path: from_path.clone(),
                        to_path: path.clone(),
                        from_commit: subtree_change_sources
                            .get(from_cs_id)
                            .ok_or_else(|| anyhow!("Subtree copy source {} not found", from_cs_id))?
                            .clone(),
                    });
                }
                SubtreeChange::SubtreeDeepCopy(SubtreeDeepCopy {
                    from_path,
                    from_cs_id,
                }) => {
                    deep_copies.push(HgSubtreeDeepCopy {
                        from_path: from_path.clone(),
                        to_path: path.clone(),
                        from_commit: subtree_change_sources
                            .get(from_cs_id)
                            .ok_or_else(|| anyhow!("Subtree copy source {} not found", from_cs_id))?
                            .clone(),
                    });
                }
                SubtreeChange::SubtreeMerge(SubtreeMerge {
                    from_path,
                    from_cs_id,
                }) => {
                    merges.push(HgSubtreeMerge {
                        from_path: from_path.clone(),
                        to_path: path.clone(),
                        from_commit: subtree_change_sources
                            .get(from_cs_id)
                            .ok_or_else(|| anyhow!("Subtree copy source {} not found", from_cs_id))?
                            .clone(),
                    });
                }
                SubtreeChange::SubtreeImport(SubtreeImport {
                    from_path,
                    from_commit,
                    from_repo_url,
                }) => {
                    imports.push(HgSubtreeImport {
                        from_path: from_path.clone(),
                        to_path: path.clone(),
                        from_commit: from_commit.clone(),
                        url: from_repo_url.clone(),
                    });
                }
            }
        }
        Ok(Some(HgSubtreeChanges {
            copies,
            deep_copies,
            merges,
            imports,
        }))
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HgSubtreeChangesVersion {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    copies: Vec<HgSubtreeCopy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    deepcopies: Vec<HgSubtreeDeepCopy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    merges: Vec<HgSubtreeMerge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    imports: Vec<HgSubtreeImport>,
    #[serde(rename = "v", with = "version")]
    #[allow(unused)]
    version: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct HgSubtreeCopy {
    #[serde(with = "hg_changeset_id")]
    pub from_commit: HgChangesetId,
    #[serde(with = "mpath")]
    pub from_path: MPath,
    #[serde(with = "mpath")]
    pub to_path: MPath,
}

impl HgSubtreeCopy {
    pub async fn to_manifest_replacement(
        &self,
        ctx: &CoreContext,
        blobstore: &Arc<dyn Blobstore>,
    ) -> Result<ManifestParentReplacement<HgManifestId, (FileType, HgFileNodeId)>> {
        let entry = self
            .from_commit
            .load(ctx, blobstore)
            .await?
            .manifestid()
            .find_entry(ctx.clone(), blobstore.clone(), self.from_path.clone())
            .await?
            .ok_or_else(|| {
                anyhow!(
                    "Subtree copy source {}:{} not found",
                    self.from_commit,
                    self.from_path
                )
            })?;
        Ok(ManifestParentReplacement {
            path: self.to_path.clone(),
            replacements: vec![entry],
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct HgSubtreeDeepCopy {
    #[serde(with = "hg_changeset_id")]
    pub from_commit: HgChangesetId,
    #[serde(with = "mpath")]
    pub from_path: MPath,
    #[serde(with = "mpath")]
    pub to_path: MPath,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct HgSubtreeMerge {
    #[serde(with = "hg_changeset_id")]
    pub from_commit: HgChangesetId,
    #[serde(with = "mpath")]
    pub from_path: MPath,
    #[serde(with = "mpath")]
    pub to_path: MPath,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct HgSubtreeImport {
    pub from_commit: String,
    #[serde(with = "mpath")]
    pub from_path: MPath,
    #[serde(with = "mpath")]
    pub to_path: MPath,
    pub url: String,
}

mod version {
    use serde::Deserialize;
    use serde::de::Error;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let version = u64::deserialize(deserializer)?;
        if version != 1 {
            return Err(D::Error::custom(format!(
                "Unsupported version of subtree changes: {}",
                version
            )));
        }
        Ok(version)
    }

    pub fn serialize<S>(version: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u64(*version)
    }
}

mod mpath {
    use mononoke_types::MPath;
    use serde::Deserialize;
    use serde::de::Error;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<MPath, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let path = String::deserialize(deserializer)?;
        MPath::new(path.as_bytes())
            .map_err(|e| D::Error::custom(format!("Invalid path: {path}: {e}")))
    }

    pub fn serialize<S>(path: &MPath, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let path = path.to_string();
        serializer.serialize_str(&path)
    }
}

mod hg_changeset_id {
    use std::str::FromStr;

    use serde::Deserialize;
    use serde::de::Error;

    use crate::HgChangesetId;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HgChangesetId, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let id = String::deserialize(deserializer)?;
        HgChangesetId::from_str(&id)
            .map_err(|e| D::Error::custom(format!("Invalid changeset id: {id}: {e}")))
    }

    pub fn serialize<S>(id: &HgChangesetId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let id = id.to_string();
        serializer.serialize_str(&id)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use mononoke_macros::mononoke;
    use mononoke_types::MPath;

    use super::*;

    fn compare(changes: HgSubtreeChanges, expected: &str) {
        let json = changes.to_json().unwrap();
        assert_eq!(json, expected);
        let parsed = HgSubtreeChanges::from_json(json.as_bytes()).unwrap();
        assert_eq!(changes, parsed);
    }

    #[mononoke::test]
    fn test_roundtrip() {
        compare(HgSubtreeChanges::default(), "[]");
        compare(
            HgSubtreeChanges {
                copies: vec![HgSubtreeCopy {
                    from_path: MPath::new("a").unwrap(),
                    to_path: MPath::new("b").unwrap(),
                    from_commit: HgChangesetId::from_str(
                        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    )
                    .unwrap(),
                }],
                ..Default::default()
            },
            r##"[{"copies":[{"from_commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","from_path":"a","to_path":"b"}],"v":1}]"##,
        );
        compare(
            HgSubtreeChanges {
                copies: vec![
                    HgSubtreeCopy {
                        from_path: MPath::new("a").unwrap(),
                        to_path: MPath::new("b").unwrap(),
                        from_commit: HgChangesetId::from_str(
                            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        )
                        .unwrap(),
                    },
                    HgSubtreeCopy {
                        from_path: MPath::new("c").unwrap(),
                        to_path: MPath::new("d").unwrap(),
                        from_commit: HgChangesetId::from_str(
                            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                        )
                        .unwrap(),
                    },
                ],
                deep_copies: vec![HgSubtreeDeepCopy {
                    from_path: MPath::new("e").unwrap(),
                    to_path: MPath::new("f").unwrap(),
                    from_commit: HgChangesetId::from_str(
                        "cccccccccccccccccccccccccccccccccccccccc",
                    )
                    .unwrap(),
                }],
                merges: vec![HgSubtreeMerge {
                    from_path: MPath::new("g").unwrap(),
                    to_path: MPath::new("h").unwrap(),
                    from_commit: HgChangesetId::from_str(
                        "dddddddddddddddddddddddddddddddddddddddd",
                    )
                    .unwrap(),
                }],
                imports: vec![HgSubtreeImport {
                    from_path: MPath::new("i").unwrap(),
                    to_path: MPath::new("j").unwrap(),
                    from_commit: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_string(),
                    url: "other:repo".to_string(),
                }],
            },
            concat!(
                r##"[{"copies":[{"from_commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","from_path":"a","to_path":"b"},"##,
                r##"{"from_commit":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","from_path":"c","to_path":"d"}],"##,
                r##""deepcopies":[{"from_commit":"cccccccccccccccccccccccccccccccccccccccc","from_path":"e","to_path":"f"}],"##,
                r##""merges":[{"from_commit":"dddddddddddddddddddddddddddddddddddddddd","from_path":"g","to_path":"h"}],"##,
                r##""imports":[{"from_commit":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","from_path":"i","to_path":"j","url":"other:repo"}],"##,
                r##""v":1}]"##
            ),
        );
    }

    #[mononoke::test]
    fn test_unsupported_version() {
        let json = r##"[{"copies":[{"from_commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","from_path":"a","to_path":"b"}],"v":2}]"##;
        let changes = HgSubtreeChanges::from_json(json.as_bytes());
        assert!(changes.is_err());
    }
}
